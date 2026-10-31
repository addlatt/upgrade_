"""The test manager's own cases: a made-up machine (rule #5, the logic level).

    python3 -m unittest rig/oneclick/test_oneclick.py
"""

import json
import re
import os
import shutil
import stat
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import oneclick  # noqa: E402
import transport as tr  # noqa: E402


class FakeLink:
    """A machine that answers from a script: probe() pops answers in order
    (the last one repeats); run() matches a substring of the command."""

    def __init__(self, probes=("windows",), runs=None, files=None):
        self.probes, self.runs, self.files = list(probes), runs or {}, files or {}
        self.commands = []

    def describe(self):
        return "fake"

    def probe(self):
        return self.probes.pop(0) if len(self.probes) > 1 else self.probes[0]

    def run(self, script, os_kind, timeout=600, stdin=""):
        self.commands.append(script)
        self.stdins = getattr(self, "stdins", []) + [stdin]
        for key, res in self.runs.items():
            if key in script:
                return res
        return tr.Result(1, "", "no answer scripted")

    def pull(self, remote, local, os_kind, timeout=1800):
        if remote not in self.files:
            return tr.Result(1, "", f"no such file {remote}")
        with open(local, "w", encoding="utf-8") as f:
            f.write(self.files[remote])
        return tr.Result(0, "", "")


class Clock:
    def __init__(self):
        self.t = 0.0

    def __call__(self):
        return self.t

    def sleep(self, s):
        self.t += s


SCAN_OUT = "SCAN_RC=0\nREPORT=E:\\upgrade_\\reports\\upgrade-report-x.json\nSTICK=E:\n"


def look_link(level="YELLOW"):
    return FakeLink(runs={
        "Win32_BIOS": tr.Result(0, '{"manufacturer": "Acer", "bios_version": "V1.21"}', ""),
        "upgrade-scan.exe": tr.Result(0, SCAN_OUT, ""),
    }, files={
        "E:\\upgrade_\\reports\\upgrade-report-x.json": json.dumps({"Verdict": {"Level": level}}),
        "E:\\upgrade_\\reports\\upgrade-report-x.txt": "report",
        "E:\\machine-capture.json": "{}",
    })


def make_run(tmp, stages, heartbeat=300):
    d = os.path.join(tmp, "run")
    os.makedirs(d)
    plan = {"machine": "fake", "heartbeat_s": heartbeat, "stages": stages}
    with open(os.path.join(d, "run.json"), "w") as f:
        json.dump({"transport": "ssh:fake", "plan": plan}, f)
    return d


def journal(d):
    return oneclick.Journal(os.path.join(d, "journal.jsonl")).entries()


class Writer:
    writes, needs = True, ()

    def __init__(self, word="done", expected=True, boom=None):
        self.word, self.expected, self.boom = word, expected, boom
        self.saw_start_first = None

    def run(self, ctx, args):
        last = ctx.journal.entries()[-1]
        self.saw_start_first = last["event"] == "stage-start"
        if self.boom:
            raise self.boom
        return oneclick.Outcome(self.word, self.expected)


class NextActionTests(unittest.TestCase):
    plan = {"stages": [{"stage": "look"}, {"stage": "writer-x"}]}

    def setUp(self):
        oneclick.STAGES["writer-x"] = Writer()

    def tearDown(self):
        oneclick.STAGES.pop("writer-x", None)

    def test_a_fresh_run_starts_at_the_first_stage(self):
        self.assertEqual(oneclick.next_action(self.plan, []), ("run", 0))

    def test_an_expected_stage_moves_on_and_the_last_one_finishes(self):
        e = [{"event": "stage-done", "index": 0, "expected": True}]
        self.assertEqual(oneclick.next_action(self.plan, e), ("run", 1))
        e.append({"event": "stage-done", "index": 1, "expected": True})
        self.assertEqual(oneclick.next_action(self.plan, e), ("finished", None))

    def test_an_unexpected_result_stops_the_chain(self):
        e = [{"event": "stage-done", "index": 0, "expected": False, "word": "scanned-unknown"}]
        a, why = oneclick.next_action(self.plan, e)
        self.assertEqual(a, "stop")
        self.assertIn("scanned-unknown", why)

    def test_an_interrupted_read_only_stage_runs_again(self):
        e = [{"event": "stage-start", "index": 0}]
        self.assertEqual(oneclick.next_action(self.plan, e), ("run", 0))

    def test_an_interrupted_writer_is_never_run_again(self):
        e = [{"event": "stage-done", "index": 0, "expected": True}, {"event": "stage-start", "index": 1}]
        a, why = oneclick.next_action(self.plan, e)
        self.assertEqual(a, "stop")
        self.assertIn("state is unknown", why)

    def test_a_stage_built_since_its_not_built_stop_runs_and_nothing_else_reopens(self):
        e = [{"event": "stage-done", "index": 0, "stage": "look", "expected": True},
             {"event": "stage-start", "index": 1, "stage": "writer-x"},
             {"event": "stage-done", "index": 1, "stage": "writer-x", "word": "not-built", "expected": False},
             {"event": "run-stopped", "why": "not built"}]
        self.assertEqual(oneclick.next_action(self.plan, e), ("run", 1))
        oneclick.STAGES.pop("writer-x")
        self.assertEqual(oneclick.next_action(self.plan, e)[0], "stop", "still not built: still stopped")
        oneclick.STAGES["writer-x"] = Writer()
        e[2]["word"] = "job-refused"
        self.assertEqual(oneclick.next_action(self.plan, e)[0], "stop", "a real stop never reopens")

    def test_a_run_stopped_for_a_missing_approval_goes_on_only_once_it_is_given(self):
        e = [{"event": "stage-done", "index": 0, "stage": "look", "expected": True},
             {"event": "run-stopped", "why": "stage 1 (writer-x) needs an approval this run was not given: erase"}]
        self.assertEqual(oneclick.next_action(self.plan, e)[0], "stop")
        e.append({"event": "consent-given", "keys": ["erase"]})
        self.assertEqual(oneclick.next_action(self.plan, e), ("run", 1))

    def test_a_stage_with_an_owner_step_waits_until_the_owner_is_done(self):
        plan = {"stages": [{"stage": "look"}, {"stage": "writer-x", "owner_step": "swap the sticks"}]}
        e = [{"event": "stage-done", "index": 0, "stage": "look", "expected": True}]
        self.assertEqual(oneclick.next_action(plan, e), ("wait", 1))
        e.append({"event": "owner-done", "index": 0})
        self.assertEqual(oneclick.next_action(plan, e), ("wait", 1), "done for another stage is not done for this one")
        e.append({"event": "owner-done", "index": 1})
        self.assertEqual(oneclick.next_action(plan, e), ("run", 1))

    def test_a_waiting_run_starts_nothing_and_done_lets_it_go_on(self):
        tmp = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, tmp)
        d = make_run(tmp, [{"stage": "look"}, {"stage": "writer-x", "owner_step": "swap the sticks"}])
        oneclick.Journal(os.path.join(d, "journal.jsonl")).write("stage-done", index=0, stage="look", word="scanned-red", expected=True)
        self.assertEqual(oneclick.run_chain(d, link=FakeLink(), out=lambda *_: None), 3)
        self.assertEqual(oneclick.run_chain(d, link=FakeLink(), out=lambda *_: None), 3)
        e = oneclick.Journal(os.path.join(d, "journal.jsonl")).entries()
        self.assertFalse([x for x in e if x["event"] == "stage-start"])
        self.assertEqual(len([x for x in e if x["event"] == "owner-wait"]), 1, "the wait is written once")
        self.assertEqual(oneclick.done(d, out=lambda *_: None), 0)
        self.assertEqual(oneclick.done(d, out=lambda *_: None), 2, "nothing waits any more")
        self.assertEqual(oneclick.next_action({"stages": [{"stage": "look"}, {"stage": "writer-x", "owner_step": "x"}]},
                                              oneclick.Journal(os.path.join(d, "journal.jsonl")).entries()), ("run", 1))

    def test_the_aspire_plan_swaps_the_sticks_around_the_walk_away(self):
        with open(os.path.join(oneclick.REPO, "rig", "oneclick", "plans", "aspire.json")) as f:
            stages = json.load(f)["stages"]
        names = [st["stage"] for st in stages]
        self.assertIn("owner_step", stages[names.index("walk-away")])
        self.assertIn("owner_step", stages[names.index("walk-away") + 1])

    def test_a_stopped_run_stays_stopped(self):
        e = [{"event": "run-stopped", "why": "x"}]
        self.assertEqual(oneclick.next_action(self.plan, e), ("stop", "x"))


class WaitTests(unittest.TestCase):
    def test_silence_past_the_limit_is_a_hang_with_heartbeats_written(self):
        tmp = tempfile.mkdtemp()
        try:
            c = Clock()
            j = oneclick.Journal(os.path.join(tmp, "j"))
            ctx = oneclick.Context(tmp, {"heartbeat_s": 300}, {}, FakeLink(probes=(None,)), j, c.sleep, c)
            self.assertIsNone(ctx.wait_for("the installer", lambda: False, limit_s=1000, every_s=30))
            ev = [e["event"] for e in j.entries()]
            self.assertEqual(ev[-1], "hang")
            self.assertEqual(ev.count("heartbeat"), 4, "at 0, 300, 600 and 900 s")
            self.assertIsNone(j.entries()[0]["answered"])
        finally:
            shutil.rmtree(tmp)

    def test_an_answer_ends_the_wait(self):
        c = Clock()
        tmp = tempfile.mkdtemp()
        try:
            ctx = oneclick.Context(tmp, {}, {}, FakeLink(), oneclick.Journal(os.path.join(tmp, "j")), c.sleep, c)
            self.assertEqual(ctx.wait_for("x", lambda: c.t >= 90 and "linux", limit_s=600), "linux")
        finally:
            shutil.rmtree(tmp)


class ChainTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp()

    def tearDown(self):
        shutil.rmtree(self.tmp)
        oneclick.STAGES.pop("writer-x", None)

    def test_look_runs_records_and_the_unbuilt_stage_stops_the_chain(self):
        d = make_run(self.tmp, [{"stage": "look", "expect": ["YELLOW", "RED"]}, {"stage": "no-such-stage"}])
        rc = oneclick.run_chain(d, link=look_link("YELLOW"), out=lambda *_: None)
        self.assertEqual(rc, 1)
        done = [e for e in journal(d) if e["event"] == "stage-done"]
        self.assertEqual([(e["word"], e["expected"]) for e in done], [("scanned-yellow", True), ("not-built", False)])
        look = os.path.join(d, "stages", "01-look")
        for n in ("facts.json", "scan.out", "report.json", "report.txt", "machine-capture.json"):
            self.assertTrue(os.path.exists(os.path.join(look, n)), n)
        with open(os.path.join(look, "SHA256SUMS")) as f:
            sums = f.read()
        self.assertEqual(len(sums.strip().splitlines()), 5)
        # running it again does not re-run anything: the run is stopped
        self.assertEqual(oneclick.run_chain(d, link=look_link(), out=lambda *_: None), 1)
        self.assertEqual(sum(1 for e in journal(d) if e["event"] == "stage-start"), 2)

    def test_a_verdict_the_plan_does_not_expect_stops(self):
        d = make_run(self.tmp, [{"stage": "look", "expect": ["GREEN"]}])
        self.assertEqual(oneclick.run_chain(d, link=look_link("RED"), out=lambda *_: None), 1)
        self.assertIn("scanned-red", [e for e in journal(d) if e["event"] == "run-stopped"][0]["why"])

    def test_the_wrong_system_answering_is_not_a_pass(self):
        d = make_run(self.tmp, [{"stage": "look"}])
        oneclick.run_chain(d, link=FakeLink(probes=("linux",)), out=lambda *_: None)
        self.assertEqual([e["word"] for e in journal(d) if e["event"] == "stage-done"], ["wrong-system"])

    def test_two_sticks_named_UPGV0_refuse(self):
        d = make_run(self.tmp, [{"stage": "look"}])
        link = look_link()
        link.runs["upgrade-scan.exe"] = tr.Result(3, "STICKS=2\n", "")
        oneclick.run_chain(d, link=link, out=lambda *_: None)
        self.assertEqual([e["word"] for e in journal(d) if e["event"] == "stage-done"], ["stick-not-found"])

    def test_the_journal_line_is_written_before_the_stage_starts(self):
        w = Writer()
        oneclick.STAGES["writer-x"] = w
        d = make_run(self.tmp, [{"stage": "writer-x"}])
        self.assertEqual(oneclick.run_chain(d, link=FakeLink(), out=lambda *_: None), 0)
        self.assertTrue(w.saw_start_first)

    def test_a_manager_error_is_a_stop_never_a_pass(self):
        oneclick.STAGES["writer-x"] = Writer(boom=RuntimeError("bug"))
        d = make_run(self.tmp, [{"stage": "writer-x"}])
        self.assertEqual(oneclick.run_chain(d, link=FakeLink(), out=lambda *_: None), 1)
        self.assertEqual([e["word"] for e in journal(d) if e["event"] == "stage-done"], ["manager-error"])

    def test_a_writer_killed_mid_stage_is_not_run_again_on_resume(self):
        oneclick.STAGES["writer-x"] = Writer(boom=KeyboardInterrupt())
        d = make_run(self.tmp, [{"stage": "writer-x"}])
        with self.assertRaises(KeyboardInterrupt):
            oneclick.run_chain(d, link=FakeLink(), out=lambda *_: None)
        oneclick.STAGES["writer-x"] = Writer()
        self.assertEqual(oneclick.run_chain(d, link=FakeLink(), out=lambda *_: None), 1)
        self.assertEqual(sum(1 for e in journal(d) if e["event"] == "stage-start"), 1)

    def test_a_read_only_stage_tried_again_gets_a_new_folder(self):
        d = make_run(self.tmp, [{"stage": "look"}])
        os.makedirs(os.path.join(d, "stages", "01-look"))
        oneclick.Journal(os.path.join(d, "journal.jsonl")).write("stage-start", index=0, stage="look", folder="01-look")
        oneclick.run_chain(d, link=look_link(), out=lambda *_: None)
        self.assertTrue(os.path.isdir(os.path.join(d, "stages", "01-look-try2")))


def keep_link(convert_out, probes=("windows",)):
    link = FakeLink(probes=probes, runs={
        "STICK=$L": tr.Result(0, "STICK=E:\n", ""),
        "COLLECTED=": tr.Result(0, "COLLECTED=C:\\upgrade_oneclick\\collect\\T\n", ""),
        "explorer.exe": tr.Result(0, "SIGNED_IN\n", ""),
        "upgrade-job.exe write": tr.Result(0, "  written: E:\\upgrade_\\job.json\nDESKTOP_RC=0\n", ""),
        "upgrade-prologue.exe start": tr.Result(0, convert_out, ""),
        "bench": tr.Result(0, "", ""),
    }, files={"E:\\upgrade_\\job.json": "{}", "C:\\upgrade_oneclick\\collect\\T": "x",
              "E:\\upgrade_\\outcome.json": '{"status": "stopped"}'})
    link.kind = "ssh"
    link.push = lambda *a, **k: tr.Result(0, "", "")
    return link


class KeepWindowsTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp()

    def tearDown(self):
        shutil.rmtree(self.tmp)

    def ctx(self, link, clock):
        d = os.path.join(self.tmp, "stage")
        os.makedirs(d)
        c = oneclick.Context(self.tmp, {"heartbeat_s": 300}, {"password": "pw-for-the-test"}, link,
                             oneclick.Journal(os.path.join(self.tmp, "j")), clock.sleep, clock)
        c.stage_dir = d
        return c

    def test_a_prologue_that_does_not_reach_its_restart_is_a_stop_with_the_record_kept(self):
        link = keep_link("  stopped at revalidate: the job is from another computer\n")
        o = oneclick.KeepWindows().run(self.ctx(link, Clock()), {})
        self.assertEqual((o.word, o.expected), ("prologue-stopped", False))
        self.assertTrue(os.path.exists(os.path.join(self.tmp, "stage", "outcome-stopped.json")))

    def test_silence_after_the_restart_is_a_hang_never_a_pass(self):
        link = keep_link("  restarting in 15 s: starting the installer from the USB stick\n", probes=("windows", None))
        c = Clock()
        o = oneclick.KeepWindows().run(self.ctx(link, c), {"limit_s": 3600})
        self.assertEqual((o.word, o.expected), ("hang-install", False))
        self.assertGreaterEqual(c.t, 3600)

    def test_the_password_hash_reaches_the_machine_and_the_password_never_does(self):
        link = keep_link("  stopped\n")
        pushed = []
        link.push = lambda local, remote, kind, **k: (pushed.append(__import__("pathlib").Path(local).read_text()), tr.Result(0, "", ""))[1]
        oneclick.KeepWindows().run(self.ctx(link, Clock()), {})
        self.assertTrue(pushed and pushed[0].startswith("$6$"))
        self.assertFalse(any("pw-for-the-test" in c for c in link.commands))
        self.assertFalse(os.path.exists(os.path.join(self.tmp, "stage", ".hash")), "the local hash file is removed")


class RollbackTests(KeepWindowsTests):
    def rb_link(self, after):
        link = FakeLink(probes=("linux", "linux", after), runs={
            "base64 -d": tr.Result(0, '{"result": "plan"}', ""),
        })
        state = {"n": 0, "boot": 0, "rebooted": False}
        def run(script, kind, timeout=600):
            link.commands.append(script)
            if "boot_id" in script or "LastBootUpTime" in script:
                return tr.Result(0, f"boot-{state['boot']}", "")
            state["n"] += 1
            if "c3lzdGVtZC1ydW4" in script:   # base64 of 'systemd-run': the restart asked for
                state["rebooted"] = True
            if state["n"] == 2:   # the real rollback
                return tr.Result(0, '{"result": "done"}', "")
            return tr.Result(0, '{"result": "plan"}', "")
        def probe():
            if len(link.probes) > 1:
                return link.probes.pop(0)   # the boot that was already up
            if state["rebooted"]:
                state["boot"] = 1           # only the last answer comes from a new boot
            return link.probes[0]
        link.probe = probe
        link.run = run
        link.pull = lambda *a, **k: tr.Result(0, "", "")
        return link

    def test_linux_answering_first_after_the_rollback_is_a_fail(self):
        o = oneclick.Rollback().run(self.ctx(self.rb_link("linux"), Clock()), {})
        self.assertEqual((o.word, o.expected), ("windows-not-first", False))

    def test_the_old_linux_still_answering_is_not_the_new_boot(self):
        # probes: the start (linux), the old boot still up (linux, same boot_id), then the new boot
        o = oneclick.Rollback().run(self.ctx(self.rb_link("linux"), Clock()), {})
        self.assertEqual(o.word, "windows-not-first", "judged on the NEW boot's answer, not the old one's")

    def test_silence_after_the_rollback_is_a_hang(self):
        o = oneclick.Rollback().run(self.ctx(self.rb_link(None), Clock()), {})
        self.assertEqual((o.word, o.expected), ("hang-after-rollback", False))


class KickoffTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp()
        self._seed = tr.seed_follow
        tr.seed_follow = lambda *a, **k: False
        self.addCleanup(setattr, tr, "seed_follow", self._seed)
        w = Writer()
        w.needs = ("erase", "password")
        oneclick.STAGES["writer-x"] = w
        self.plan = os.path.join(self.tmp, "plan.json")
        with open(self.plan, "w") as f:
            json.dump({"machine": "fake", "stages": [{"stage": "writer-x"}]}, f)

    def tearDown(self):
        shutil.rmtree(self.tmp)
        oneclick.STAGES.pop("writer-x", None)

    def test_a_sentence_typed_wrong_starts_nothing(self):
        d = oneclick.kickoff(self.plan, "ssh:fake", ask=lambda _: "I confirm", out=lambda *_: None, root=self.tmp)
        self.assertIsNone(d)
        self.assertEqual(sorted(os.listdir(self.tmp)), ["plan.json"])

    def test_the_sentence_typed_in_full_is_kept_where_only_the_owner_reads_it(self):
        s = oneclick.sentence("erase")
        d = oneclick.kickoff(self.plan, "ssh:fake", ask=lambda _: s, out=lambda *_: None, root=self.tmp)
        sec = os.path.join(d, "secrets.json")
        self.assertEqual(stat.S_IMODE(os.stat(sec).st_mode), 0o600)
        with open(sec) as f:
            kept = json.load(f)
        self.assertEqual(kept["erase"], s)
        self.assertGreaterEqual(len(kept["password"]), 16)

    def test_a_sentence_the_plan_names_is_asked_at_kickoff_and_kept(self):
        with open(self.plan, "w") as f:
            json.dump({"machine": "fake", "stages": [{"stage": "writer-x", "needs": ["data-loss"]}]}, f)
        shown, asked = [], []
        sentences = {oneclick.sentence(k): k for k in ("erase", "data-loss")}

        def ask(_):   # types whichever sentence the manager just showed
            s = [t for t in sentences if t in shown[-1]][0]
            asked.append(sentences[s])
            return s
        d = oneclick.kickoff(self.plan, "ssh:fake", ask=ask, out=shown.append, root=self.tmp)
        self.assertEqual(sorted(asked), ["data-loss", "erase"])
        with open(os.path.join(d, "secrets.json")) as f:
            self.assertEqual(json.load(f)["data-loss"], oneclick.sentence("data-loss"))

    def test_the_data_loss_sentence_typed_wrong_starts_nothing(self):
        with open(self.plan, "w") as f:
            json.dump({"machine": "fake", "stages": [{"stage": "writer-x", "needs": ["data-loss"]}]}, f)
        erase = oneclick.sentence("erase")
        d = oneclick.kickoff(self.plan, "ssh:fake", ask=lambda _: erase, out=lambda *_: None, root=self.tmp)
        self.assertIsNone(d, "the erase sentence is not the data-loss sentence")
        self.assertEqual(sorted(os.listdir(self.tmp)), ["plan.json"])

    def test_a_plan_naming_an_approval_the_manager_does_not_know_refuses(self):
        with open(self.plan, "w") as f:
            json.dump({"machine": "fake", "stages": [{"stage": "writer-x", "needs": ["override"]}]}, f)
        with self.assertRaises(SystemExit):
            oneclick.kickoff(self.plan, "ssh:fake", ask=lambda _: "", out=lambda *_: None, root=self.tmp)

    def test_a_run_missing_a_plan_named_sentence_stops_before_the_stage(self):
        d = make_run(self.tmp, [{"stage": "writer-x", "needs": ["data-loss"]}])
        with open(os.path.join(d, "secrets.json"), "w") as f:
            json.dump({"password": "x" * 16, "erase": oneclick.sentence("erase")}, f)
        self.assertEqual(oneclick.run_chain(d, link=FakeLink(), out=lambda *_: None), 1)
        e = oneclick.Journal(os.path.join(d, "journal.jsonl")).entries()
        self.assertFalse([x for x in e if x["event"] == "stage-start"])
        self.assertIn("data-loss", e[-1]["why"])

    def test_the_aspire_plan_asks_for_the_data_loss_sentence_wherever_a_job_is_written(self):
        with open(os.path.join(oneclick.REPO, "rig", "oneclick", "plans", "aspire.json")) as f:
            plan = json.load(f)
        for st in plan["stages"]:
            if st["stage"] in ("keep-windows", "clean-slate", "cancel-countdown", "refused-before-countdown"):
                self.assertIn("data-loss", oneclick.stage_needs(st), st["stage"])
        self.assertIn("data-loss", oneclick.plan_needs(plan["stages"]))

    def test_a_plan_with_a_test_password_uses_it(self):
        with open(self.plan, "w") as f:
            json.dump({"machine": "fake", "test_password": "password", "stages": [{"stage": "writer-x"}]}, f)
        d = oneclick.kickoff(self.plan, "ssh:fake", ask=lambda _: oneclick.sentence("erase"), out=lambda *_: None, root=self.tmp)
        with open(os.path.join(d, "secrets.json")) as f:
            self.assertEqual(json.load(f)["password"], "password")

    def test_a_tail_run_keeps_the_earlier_runs_password(self):
        s = oneclick.sentence("erase")
        first = oneclick.kickoff(self.plan, "ssh:fake", ask=lambda _: s, out=lambda *_: None, root=self.tmp)
        tail = oneclick.kickoff(self.plan, "ssh:fake", ask=lambda _: s, out=lambda *_: None, root=os.path.join(self.tmp, "t"), password_from=first)
        with open(os.path.join(first, "secrets.json")) as f, open(os.path.join(tail, "secrets.json")) as g:
            self.assertEqual(json.load(f)["password"], json.load(g)["password"])

    def test_a_sentence_missing_from_the_source_refuses(self):
        os.makedirs(os.path.join(self.tmp, "evaluate/job/src"))
        with open(os.path.join(self.tmp, "evaluate/job/src/lib.rs"), "w") as f:
            f.write("nothing here")
        with self.assertRaises(SystemExit):
            oneclick.sentence("erase", repo=self.tmp)


class WalkAwayStickTests(unittest.TestCase):
    """The walk-away picks its stick from `go-back sticks`, as the window does;
    the plan's drives are the ones it erases (the Aspire, 2026-10-28)."""

    PLAN = json.dumps({"drives": [{"name": "sdb", "role": "system", "serial": "EI8", "size_bytes": 256060514304}], "language": "en-us"})

    def run_with(self, sticks):
        import base64
        tmp = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, tmp)
        link = FakeLink(probes=("linux",))
        link.kind = "ssh"
        sent = []

        def run(script, kind, timeout=600):
            m = re.search(r"echo (\S+) \| base64 -d\)", script)
            body = base64.b64decode(m.group(1)).decode() if m else script
            sent.append(body)
            if "walkaway plan" in body:
                return tr.Result(0, self.PLAN, "")
            if "go-back sticks" in body:
                return tr.Result(0, json.dumps(sticks), "")
            if "walkaway prepare" in body:
                return tr.Result(1, '{"result": "stopped"}', "")
            return tr.Result(0, "user", "")
        link.run = run
        c = oneclick.Context(tmp, {"heartbeat_s": 300}, {"password": "p", "walkaway": "s", "walkaway-stick": "UDisk", "walkaway-account": "a"},
                             link, oneclick.Journal(os.path.join(tmp, "j")))
        c.stage_dir = tmp
        return oneclick.WalkAway().run(c, {}), sent

    def test_the_offered_stick_with_the_typed_word_is_the_one_prepared(self):
        o, sent = self.run_with([{"confirm_word": "HFS256G39TND-N21", "offered": False, "serial": "EI8", "size_bytes": 1},
                                 {"confirm_word": "UDisk", "offered": True, "serial": "General_UDisk-0:0", "size_bytes": 15728640000}])
        self.assertEqual(o.word, "walkaway-not-ready", "it reached prepare")
        prep = [b for b in sent if "walkaway prepare" in b][0]
        self.assertIn("--serial 'General_UDisk-0:0' --size 15728640000 --typed 'UDisk'", prep)
        self.assertIn("--min-bytes 14000000000", [b for b in sent if "go-back sticks" in b][0])

    def test_a_refused_stick_is_never_prepared_even_with_the_word(self):
        o, sent = self.run_with([{"confirm_word": "UDisk", "offered": False, "serial": "x", "size_bytes": 1}])
        self.assertEqual(o.word, "walkaway-stick-not-confirmed")
        self.assertFalse([b for b in sent if "walkaway prepare" in b])

    def test_two_offered_sticks_with_one_word_are_never_guessed(self):
        both = {"confirm_word": "UDisk", "offered": True, "serial": "General_UDisk-0:0", "size_bytes": 15728640000}
        o, sent = self.run_with([both, dict(both)])
        self.assertEqual(o.word, "walkaway-stick-not-confirmed")
        self.assertFalse([b for b in sent if "walkaway prepare" in b])


class HoldAwakeTests(unittest.TestCase):
    def chain(self, stage):
        tmp = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, tmp)
        d = make_run(tmp, [stage])
        calls = []
        held, released = oneclick.hold_awake, oneclick.release_awake
        oneclick.hold_awake = lambda ctx: (calls.append("hold"), True)[1]
        oneclick.release_awake = lambda ctx: calls.append("release")
        try:
            oneclick.run_chain(d, link=look_link(), out=lambda *_: None)
        finally:
            oneclick.hold_awake, oneclick.release_awake = held, released
        done = [x for x in oneclick.Journal(os.path.join(d, "journal.jsonl")).entries() if x["event"] == "stage-done"][0]
        return calls, done

    def test_a_stage_without_hold_awake_takes_no_lock(self):
        calls, done = self.chain({"stage": "look"})
        self.assertEqual(calls, [])
        self.assertNotIn("sleep lock", done["notes"])

    def test_hold_awake_takes_the_lock_releases_it_and_the_row_says_the_test_did(self):
        calls, done = self.chain({"stage": "look", "hold_awake": True})
        self.assertEqual(calls, ["hold", "release"])
        self.assertIn("the TEST held a sleep lock", done["notes"])

    def test_no_linux_means_no_lock_and_says_so(self):
        ctx = oneclick.Context("/tmp", {}, {"password": "p"}, FakeLink(probes=("windows",)), oneclick.Journal(os.devnull))
        notes = []
        ctx.note = lambda text, **kv: notes.append(text)
        self.assertFalse(oneclick.hold_awake(ctx))
        self.assertIn("no sleep lock", notes[0])

    def test_a_held_lock_is_said_in_the_stage_notes(self):
        tmp = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, tmp)
        ctx = oneclick.Context(tmp, {}, {"password": "p"}, FakeLink(probes=("linux",), runs={"base64 -d": tr.Result(0, "HELD", "")}), oneclick.Journal(os.path.join(tmp, "j")))
        self.assertTrue(oneclick.hold_awake(ctx))
        self.assertIn("TEST holds a sleep lock", ctx.journal.entries()[0]["text"])

    def test_the_aspire_plans_hold_the_walk_away_awake(self):
        for name in ("aspire.json",):
            with open(os.path.join(oneclick.REPO, "rig", "oneclick", "plans", name)) as f:
                st = [x for x in json.load(f)["stages"] if x["stage"] == "walk-away"][0]
            self.assertTrue(st.get("hold_awake"), name)


class DesktopTests(unittest.TestCase):
    def test_the_command_runs_as_a_desktop_task_and_the_answer_is_its_log(self):
        link = FakeLink(runs={"upgrade-job.exe write": tr.Result(0, "#< CLIXML\n<Objs>noise</Objs>  written\nDESKTOP_RC=2\n", "")})
        r = oneclick.on_desktop(link, 'E:\\upgrade-job.exe write --acknowledge-data-loss "I confirm"', "DESKTOP_RC=", 1800)
        sent = link.commands[0]
        self.assertIn("-LogonType Interactive -RunLevel Highest", sent)
        self.assertIn("-DontStopIfGoingOnBatteries", sent)
        self.assertNotIn("I confirm", sent.split("<#")[0], "the command travels encoded")
        self.assertNotIn("CLIXML", r.out)
        self.assertIn("DESKTOP_RC=2", r.out)

    def test_a_refusing_job_writer_on_the_desktop_is_job_refused(self):
        link = keep_link("x")
        link.runs["upgrade-job.exe write"] = tr.Result(0, "REFUSED - no job written\nDESKTOP_RC=2\n", "")
        t = KeepWindowsTests()
        t.tmp = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, t.tmp)
        o = oneclick.KeepWindows().run(t.ctx(link, Clock()), {})
        self.assertEqual((o.word, o.expected), ("job-refused", False))


class SignInTests(unittest.TestCase):
    def ctx(self, answers):
        tmp = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, tmp)
        link = FakeLink()
        seq = list(answers)
        link.run = lambda script, os_kind, timeout=600: tr.Result(0, seq.pop(0) if len(seq) > 1 else seq[0], "")
        clock = Clock()
        c = oneclick.Context(tmp, {"heartbeat_s": 300}, {}, link, oneclick.Journal(os.path.join(tmp, "j")), clock.sleep, clock)
        return c

    def test_a_signed_in_desktop_goes_straight_on(self):
        c = self.ctx(["SIGNED_IN"])
        self.assertTrue(oneclick.wait_signed_in(c))
        self.assertEqual(c.journal.entries(), [])

    def test_the_sign_in_screen_waits_for_the_owner_and_says_so(self):
        c = self.ctx(["NOBODY", "NOBODY", "NOBODY", "SIGNED_IN"])
        self.assertTrue(oneclick.wait_signed_in(c))
        self.assertIn("sign in", c.journal.entries()[0]["text"])

    def test_nobody_signing_in_is_a_hang_never_a_job(self):
        c = self.ctx(["NOBODY"])
        self.assertFalse(oneclick.wait_signed_in(c, limit_s=600))
        self.assertTrue(any(e["event"] == "hang" for e in c.journal.entries()))


class TransportTests(unittest.TestCase):
    def test_windows_commands_travel_encoded(self):
        self.assertEqual(tr.encoded("a"), "YQA=")

    def test_unknown_transports_refuse(self):
        with self.assertRaises(ValueError):
            tr.from_spec("telnet:x")


ASPIRE = {"os": "windows", "model": "Aspire A515-51G", "macs": ["A0C589112233", "A0-C5-89-11-22-33"], "disks": ["FS83N008110", "WD-WCC4J1234567"]}


class SameMachineTests(unittest.TestCase):
    def test_linux_and_windows_spellings_of_one_computer_match(self):
        seen = {"os": "linux", "model": "Aspire A515-51G", "macs": ["a0:c5:89:11:22:33"], "disks": ["WD-WCC4J1234567"]}
        ok, why = tr.same_machine(ASPIRE, seen)
        self.assertTrue(ok, why)
        self.assertIn("1 network card(s) and 1 drive(s)", why)

    def test_another_model_never_matches_even_with_a_common_card(self):
        seen = dict(ASPIRE, model="Aspire A515-52")
        self.assertFalse(tr.same_machine(ASPIRE, seen)[0])

    def test_the_same_model_with_nothing_in_common_is_another_computer(self):
        seen = {"model": "Aspire A515-51G", "macs": ["a0:c5:89:99:99:99"], "disks": ["OTHERDISK01"]}
        self.assertFalse(tr.same_machine(ASPIRE, seen)[0])

    def test_empty_and_zeroed_values_never_count(self):
        known = {"model": "X", "macs": ["00-00-00-00-00-00", ""], "disks": [None, "0000_0000"]}
        self.assertFalse(tr.same_machine(known, dict(known))[0])
        self.assertFalse(tr.same_machine({"model": ""}, {"model": ""})[0])
        self.assertFalse(tr.same_machine(None, ASPIRE)[0])


class FollowTests(unittest.TestCase):
    """The machine followed across addresses and new host keys (the Aspire,
    2026-10-28/29: .211, .79, .245 and a new key at every install)."""
    def setUp(self):
        self.tmp = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, self.tmp)
        with open(os.path.join(self.tmp, "machine.json"), "w") as f:
            json.dump({"alias": "oneclick-fake", "address": "10.0.0.11", "addresses": ["10.0.0.11"], "prefix": "10.0.0", "identity": ASPIRE}, f)
        with open(os.path.join(self.tmp, "known_hosts"), "w") as f:
            f.write("oneclick-fake ssh-ed25519 OLDKEY\n")
        self.keys, self.who = {}, {}
        saved = (tr.keyscan, tr.read_identity, tr.Follow.sweep)
        self.addCleanup(lambda: (setattr(tr, "keyscan", saved[0]), setattr(tr, "read_identity", saved[1]), setattr(tr.Follow, "sweep", saved[2])))
        tr.keyscan = lambda ip: self.keys.get(ip, set())
        tr.Follow.sweep = lambda me: sorted(self.keys)

        def identity(look):   # what ssh's accept-new would write, then the machine's answer
            ip = [o.split("=", 1)[1] for o in look.opts if o.startswith("HostName=")][0]
            q = [o.split("=", 1)[1] for o in look.opts if o.startswith("UserKnownHostsFile=")][0]
            with open(q, "w") as f:
                f.writelines(f"oneclick-fake {t} {k}\n" for t, k in self.keys[ip])
            return self.who.get(ip)
        tr.read_identity = identity
        self.events = []
        self.link = tr.Follow("fake", self.tmp, clock=lambda: self.t)
        self.link.on_event = lambda ev, **kv: self.events.append((ev, kv))
        self.t = 0

    def trusted(self):
        with open(os.path.join(self.tmp, "known_hosts")) as f:
            return f.read()

    def test_every_call_uses_the_runs_own_address_and_known_hosts_with_checks_on(self):
        o = " ".join(self.link.opts)
        for want in ("ProxyCommand=none", "HostName=10.0.0.11", "HostKeyAlias=oneclick-fake", "StrictHostKeyChecking=yes",
                     f"UserKnownHostsFile={os.path.join(self.tmp, 'known_hosts')}"):
            self.assertIn(want, o)

    def test_a_known_key_at_a_new_address_is_followed(self):
        self.keys = {"10.0.0.79": {("ssh-ed25519", "OLDKEY")}}
        self.assertTrue(self.link.relocate())
        self.assertIn("HostName=10.0.0.79", self.link.opts)
        self.assertEqual(self.events[0][0], "machine-moved")
        with open(os.path.join(self.tmp, "machine.json")) as f:
            self.assertEqual(json.load(f)["address"], "10.0.0.79", "the new address outlives the manager")

    def test_a_new_key_is_trusted_only_after_it_proves_it_is_the_same_computer(self):
        self.keys = {"10.0.0.245": {("ssh-ed25519", "NEWKEY")}}
        self.who = {"10.0.0.245": {"os": "linux", "model": "Aspire A515-51G", "macs": ["a0:c5:89:11:22:33"], "disks": []}}
        self.assertTrue(self.link.relocate())
        self.assertIn("NEWKEY", self.trusted())
        self.assertIn("OLDKEY", self.trusted())
        self.assertEqual([e[0] for e in self.events], ["host-key-trusted", "machine-moved"])
        self.assertTrue([n for n in os.listdir(self.tmp) if n.startswith("follow-")], "the look that decided it is kept")
        self.assertFalse([n for n in os.listdir(self.tmp) if n.startswith("oneclick-kh-")], "the quarantine file is removed")

    def test_another_computer_with_a_new_key_is_never_trusted(self):
        self.keys = {"10.0.0.50": {("ssh-ed25519", "STRANGER")}}
        self.who = {"10.0.0.50": {"os": "linux", "model": "Some Other PC", "macs": ["a0:c5:89:11:22:33"], "disks": []}}
        self.assertFalse(self.link.relocate())
        self.assertNotIn("STRANGER", self.trusted())
        self.assertIn("HostName=10.0.0.11", self.link.opts)
        self.assertEqual(self.events[0][0], "host-key-refused")

    def test_a_silent_machine_is_looked_for_at_most_every_two_minutes(self):
        self.assertFalse(self.link.relocate())
        self.keys = {"10.0.0.79": {("ssh-ed25519", "OLDKEY")}}
        self.t = 60
        self.assertFalse(self.link.relocate(), "too soon: no sweep")
        self.t = 121
        self.assertTrue(self.link.relocate())

    def test_a_run_with_a_machine_file_follows_and_one_without_does_not(self):
        self.assertIsInstance(tr.from_spec("ssh:fake", self.tmp), tr.Follow)
        self.assertNotIsInstance(tr.from_spec("ssh:fake", tempfile.gettempdir() + "/no-such-run"), tr.Follow)


class RetryTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, self.tmp)
        self.addCleanup(oneclick.STAGES.pop, "writer-x", None)

    def stopped(self, outcome):
        class Once(Writer):
            def __init__(self):
                super().__init__()
                self.n = 0

            def run(self, ctx, args):
                self.n += 1
                return outcome if self.n == 1 else oneclick.Outcome("done", True)
        w = Once()
        oneclick.STAGES["writer-x"] = w
        d = make_run(self.tmp, [{"stage": "look"}, {"stage": "writer-x"}])
        oneclick.Journal(os.path.join(d, "journal.jsonl")).write("stage-done", index=0, stage="look", word="scanned-red", expected=True)
        self.assertEqual(oneclick.run_chain(d, link=FakeLink(), out=lambda *_: None), 1)
        return d, w

    def test_a_stage_that_changed_nothing_goes_again_in_the_same_run_with_its_fail_kept(self):
        d, w = self.stopped(oneclick.untouched("job-refused", "nobody at the desktop"))
        self.assertEqual(oneclick.retry(d, out=lambda *_: None), 0)
        self.assertEqual(oneclick.run_chain(d, link=FakeLink(), out=lambda *_: None), 0)
        done = [(e["word"], e["expected"]) for e in journal(d) if e["event"] == "stage-done" and e["index"] == 1]
        self.assertEqual(done, [("job-refused", False), ("done", True)], "the fail stays in the record")
        self.assertTrue(os.path.isdir(os.path.join(d, "stages", "02-writer-x-try2")))

    def test_a_stage_that_may_have_changed_the_machine_needs_the_owners_words(self):
        d, _ = self.stopped(oneclick.Outcome("walkaway-not-ready", False, "slept"))
        self.assertEqual(oneclick.retry(d, out=lambda *_: None), 2)
        self.assertEqual(oneclick.next_action({"stages": [{"stage": "look"}, {"stage": "writer-x"}]}, journal(d))[0], "stop")
        self.assertEqual(oneclick.retry(d, undone="settle-in go-back walkaway undo", out=lambda *_: None), 0)
        r = [e for e in journal(d) if e["event"] == "stage-retry"][0]
        self.assertEqual((r["untouched"], r["undone"]), (False, "settle-in go-back walkaway undo"))

    def test_an_interrupted_writer_is_never_retried_blind(self):
        oneclick.STAGES["writer-x"] = Writer(boom=KeyboardInterrupt())
        d = make_run(self.tmp, [{"stage": "writer-x"}])
        with self.assertRaises(KeyboardInterrupt):
            oneclick.run_chain(d, link=FakeLink(), out=lambda *_: None)
        self.assertEqual(oneclick.run_chain(d, link=FakeLink(), out=lambda *_: None), 1)
        self.assertEqual(oneclick.retry(d, out=lambda *_: None), 2)

    def test_a_running_run_has_nothing_to_retry(self):
        d = make_run(self.tmp, [{"stage": "look"}])
        self.assertEqual(oneclick.retry(d, out=lambda *_: None), 2)

    def test_a_retried_stage_that_stops_again_stops_the_run_again(self):
        d, w = self.stopped(oneclick.untouched("job-refused", "x"))
        w.n = -5   # it fails again
        w.run = lambda ctx, args: oneclick.untouched("job-refused", "again")
        oneclick.retry(d, out=lambda *_: None)
        self.assertEqual(oneclick.run_chain(d, link=FakeLink(), out=lambda *_: None), 1)
        self.assertEqual(sum(1 for e in journal(d) if e["event"] == "run-stopped"), 2)
        self.assertEqual(oneclick.retry(d, out=lambda *_: None), 0, "and can be tried once more")

    def test_early_stops_are_marked_untouched_and_a_prologue_stop_is_not(self):
        self.assertTrue(oneclick.untouched("job-refused", "x").untouched)
        link = keep_link("  stopped at shrink\n")
        t = KeepWindowsTests()
        t.tmp = self.tmp
        o = oneclick.KeepWindows().run(t.ctx(link, Clock()), {})
        self.assertEqual(o.word, "prologue-stopped")
        self.assertFalse(o.untouched)


class OwnerCheckTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, self.tmp)
        oneclick.STAGES["writer-x"] = Writer()
        self.addCleanup(oneclick.STAGES.pop, "writer-x", None)
        self.addCleanup(oneclick.OWNER_CHECKS.pop, "fake-swap", None)

    def test_a_swap_the_machine_can_see_needs_no_word_from_the_owner(self):
        seq = [False, True, False, True, True]
        oneclick.OWNER_CHECKS["fake-swap"] = lambda ctx: seq.pop(0) if seq else True
        d = make_run(self.tmp, [{"stage": "writer-x", "owner_step": "swap the sticks", "owner_check": "fake-swap"}])
        c = Clock()
        said = []
        self.assertEqual(oneclick.run_chain(d, link=FakeLink(), sleep=c.sleep, clock=c, out=said.append), 0)
        od = [e for e in journal(d) if e["event"] == "owner-done"][0]
        self.assertEqual(od["by"], "seen on the machine")
        self.assertGreaterEqual(c.t, 4 * 30, "true twice in a row, never a half-in stick")
        self.assertTrue(any("no need to say so" in x for x in said))

    def test_a_swap_not_seen_keeps_waiting_and_starts_nothing(self):
        oneclick.OWNER_CHECKS["fake-swap"] = lambda ctx: False
        d = make_run(self.tmp, [{"stage": "writer-x", "owner_step": "swap", "owner_check": "fake-swap", "owner_limit_s": 600}])
        c = Clock()
        self.assertEqual(oneclick.run_chain(d, link=FakeLink(), sleep=c.sleep, clock=c, out=lambda *_: None), 3)
        self.assertFalse([e for e in journal(d) if e["event"] in ("stage-start", "owner-done")])

    def test_the_kit_check_wants_the_kit_and_no_walk_away_stick(self):
        ctx = oneclick.Context(self.tmp, {}, {}, FakeLink(runs={"go-back.json": tr.Result(0, "KIT_ONLY\n", "")}), oneclick.Journal(os.devnull))
        self.assertTrue(oneclick.kit_stick_in(ctx))
        ctx.link = FakeLink(runs={"go-back.json": tr.Result(0, "", "")})
        self.assertFalse(oneclick.kit_stick_in(ctx))
        ctx.link = FakeLink(probes=("linux",))
        self.assertFalse(oneclick.kit_stick_in(ctx))

    def test_the_walk_away_check_wants_the_kit_out_and_one_offered_stick(self):
        one = json.dumps([{"offered": True}, {"offered": False}])
        ctx = oneclick.Context(self.tmp, {}, {"password": "p"}, FakeLink(probes=("linux",), runs={"base64 -d": tr.Result(0, one + "\n", "")}), oneclick.Journal(os.devnull))
        self.assertTrue(oneclick.walkaway_stick_in(ctx))
        ctx.link = FakeLink(probes=("linux",), runs={"base64 -d": tr.Result(0, "KIT_IN\n" + one + "\n", "")})
        self.assertFalse(oneclick.walkaway_stick_in(ctx), "the kit stick still in")
        ctx.link = FakeLink(probes=("linux",), runs={"base64 -d": tr.Result(0, json.dumps([{"offered": True}, {"offered": True}]) + "\n", "")})
        self.assertFalse(oneclick.walkaway_stick_in(ctx), "two offered: the walk-away would refuse")

    def test_the_aspire_plan_sees_both_swaps_itself(self):
        with open(os.path.join(oneclick.REPO, "rig", "oneclick", "plans", "aspire.json")) as f:
            stages = {st["stage"]: st for st in json.load(f)["stages"]}
        self.assertEqual(stages["walk-away"]["owner_check"], "walkaway-stick-in")
        self.assertEqual(stages["cancel-countdown"]["owner_check"], "kit-stick-in")

    def test_a_plan_naming_an_unknown_check_refuses(self):
        plan = os.path.join(self.tmp, "plan.json")
        with open(plan, "w") as f:
            json.dump({"machine": "fake", "stages": [{"stage": "look", "owner_step": "x", "owner_check": "no-such-check"}]}, f)
        with self.assertRaises(SystemExit):
            oneclick.kickoff(plan, "ssh:fake", ask=lambda _: "", out=lambda *_: None, root=self.tmp, follow=False)


class AutologonTests(unittest.TestCase):
    def ctx(self, answers, plan=None, secrets=None):
        tmp = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, tmp)
        link = FakeLink(probes=("windows",), runs=answers)
        link.kind = "ssh"
        c = Clock()
        bm = iter(["2026-10-31T10:00:00", "2026-10-31T10:05:00"] + ["2026-10-31T10:05:00"] * 50)
        link.runs.setdefault("LastBootUpTime", None)
        orig = link.run

        def run(script, os_kind, timeout=600, stdin=""):
            if "LastBootUpTime" in script:
                return tr.Result(0, next(bm), "")
            return orig(script, os_kind, timeout, stdin)
        link.run = run
        ctx = oneclick.Context(tmp, {"test_autologon": True} if plan is None else plan, secrets or {"windows-password": "password"}, link,
                               oneclick.Journal(os.path.join(tmp, "j")), c.sleep, c)
        return ctx

    def signed_after_restart(self):
        state = {"restarted": False}

        def explorer():
            return tr.Result(0, "SIGNED_IN" if state["restarted"] else "NOBODY", "")
        return state, explorer

    def test_the_sign_in_screen_signs_itself_in_and_says_so(self):
        ctx = self.ctx({"AutoAdminLogon": tr.Result(0, 'BEFORE={"AutoAdminLogon":"0","DefaultUserName":"bk","DefaultDomainName":null}\nAUTOLOGON=SET\n', ""),
                        "shutdown /r": tr.Result(0, "", ""), "explorer.exe": tr.Result(0, "NOBODY", "")})
        calls = {"n": 0}
        base = ctx.link.run

        def run(script, os_kind, timeout=600, stdin=""):
            if "explorer.exe" in script:
                calls["n"] += 1
                return tr.Result(0, "SIGNED_IN" if any("shutdown /r" in c for c in ctx.link.commands) else "NOBODY", "")
            return base(script, os_kind, timeout, stdin)
        ctx.link.run = run
        self.assertTrue(oneclick.wait_signed_in(ctx))
        sent = [c for c in ctx.link.commands if "AutoAdminLogon" in c][0]
        self.assertNotIn("password'", sent.replace("$pw", ""), "the password is never in the command")
        self.assertIn("password\n", ctx.link.stdins, "it travels on stdin")
        self.assertIn("UpgLsa", sent.split("FromBase64String")[0] + __import__("base64").b64decode(re.search(r"FromBase64String\('([^']+)'\)", sent).group(1)).decode(),
                      "the password is stored as the LSA secret, not registry text")
        notes = [e.get("text", "") for e in ctx.journal.entries()]
        self.assertTrue(any("signed Windows in by itself" in n for n in notes))
        self.assertTrue(oneclick.autologon_state(ctx.journal.entries()))

    def test_without_the_plans_word_the_owner_signs_in(self):
        ctx = self.ctx({"explorer.exe": tr.Result(0, "NOBODY", "")}, plan={})
        self.assertFalse(oneclick.wait_signed_in(ctx, limit_s=60))
        self.assertFalse([c for c in ctx.link.commands if "AutoAdminLogon" in c])

    def test_a_wrong_password_changes_nothing_and_falls_back_to_the_owner(self):
        ctx = self.ctx({"AutoAdminLogon": tr.Result(4, "AUTOLOGON=WRONG-PASSWORD\n", ""), "explorer.exe": tr.Result(0, "NOBODY", "")})
        self.assertFalse(oneclick.wait_signed_in(ctx, limit_s=60))
        self.assertFalse([c for c in ctx.link.commands if "shutdown /r" in c], "no restart for a sign-in that cannot work")
        self.assertFalse(oneclick.autologon_state(ctx.journal.entries()))

    def test_someone_elses_automatic_sign_in_is_left_alone(self):
        self.assertIn("AUTOLOGON=SOMEONE-ELSE", oneclick.AUTOLOGON_ON)
        self.assertIn("AUTOLOGON=PLAINTEXT-PRESENT", oneclick.AUTOLOGON_ON)

    def test_a_fresh_walk_away_account_gets_the_test_password_and_the_row_says_so(self):
        ctx = self.ctx({"AutoAdminLogon": tr.Result(0, "BEFORE={}\nFIRST_PASSWORD=CHOSEN\nAUTOLOGON=SET\n", "")})
        self.assertEqual(oneclick.autologon_on(ctx), "SET")
        self.assertTrue(any("chose the fresh Windows account's password" in e.get("text", "") for e in ctx.journal.entries()))

    def test_off_puts_back_what_was_there_and_removes_the_secret(self):
        ctx = self.ctx({"AUTOLOGON=OFF": tr.Result(0, "AUTOLOGON=OFF\n", "")})
        ctx.journal.write("autologon-set", before={"AutoAdminLogon": "0", "DefaultUserName": "bk", "DefaultDomainName": None})
        self.assertTrue(oneclick.autologon_off(ctx.link, ctx.journal, out=lambda *_: None))
        sent = ctx.link.commands[-1]
        self.assertIn("Remove-ItemProperty $k DefaultDomainName", sent)
        self.assertIn(f"FromBase64String('{oneclick.b64('bk')}')", sent)
        self.assertIn("Store('DefaultPassword', $null)", sent)
        self.assertIsNone(oneclick.autologon_state(ctx.journal.entries()))

    def test_every_stage_run_with_it_on_says_the_test_signed_in(self):
        tmp = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, tmp)
        d = make_run(tmp, [{"stage": "look"}])
        oneclick.Journal(os.path.join(d, "journal.jsonl")).write("autologon-set", before={})
        link = look_link()
        link.runs["AUTOLOGON=OFF"] = tr.Result(0, "AUTOLOGON=OFF\n", "")
        self.assertEqual(oneclick.run_chain(d, link=link, out=lambda *_: None), 0)
        done = [e for e in journal(d) if e["event"] == "stage-done"][0]
        self.assertIn("automatic sign-in was on", done["notes"])
        self.assertEqual(journal(d)[-1]["event"], "autologon-off", "a finished run switches it off")

    def test_kickoff_takes_the_windows_password_from_the_plan_or_hidden(self):
        tmp = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, tmp)
        plan = os.path.join(tmp, "plan.json")
        with open(plan, "w") as f:
            json.dump({"machine": "fake", "test_password": "password", "test_autologon": True, "windows_password": "test_password", "stages": [{"stage": "look"}]}, f)
        d = oneclick.kickoff(plan, "ssh:fake", out=lambda *_: None, root=tmp, follow=False)
        with open(os.path.join(d, "secrets.json")) as f:
            self.assertEqual(json.load(f)["windows-password"], "password")
        with open(plan, "w") as f:
            json.dump({"machine": "fake2", "test_autologon": True, "stages": [{"stage": "look"}]}, f)
        d = oneclick.kickoff(plan, "ssh:fake", out=lambda *_: None, root=tmp, follow=False, ask_secret=lambda _: "owners-own")
        with open(os.path.join(d, "secrets.json")) as f:
            self.assertEqual(json.load(f)["windows-password"], "owners-own")

    def test_the_aspire_plan_signs_itself_in_with_the_test_password(self):
        with open(os.path.join(oneclick.REPO, "rig", "oneclick", "plans", "aspire.json")) as f:
            plan = json.load(f)
        self.assertTrue(plan["test_autologon"])
        self.assertEqual(plan["windows_password"], "test_password")


if __name__ == "__main__":
    unittest.main()
