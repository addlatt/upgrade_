"""The test manager's own cases: a made-up machine (rule #5, the logic level).

    python3 -m unittest rig/oneclick/test_oneclick.py
"""

import json
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

    def run(self, script, os_kind, timeout=600):
        self.commands.append(script)
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
        "upgrade-job.exe write": tr.Result(0, "JOB_RC=0\n", ""),
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

    def test_a_sentence_missing_from_the_source_refuses(self):
        os.makedirs(os.path.join(self.tmp, "evaluate/job/src"))
        with open(os.path.join(self.tmp, "evaluate/job/src/lib.rs"), "w") as f:
            f.write("nothing here")
        with self.assertRaises(SystemExit):
            oneclick.sentence("erase", repo=self.tmp)


class TransportTests(unittest.TestCase):
    def test_windows_commands_travel_encoded(self):
        self.assertEqual(tr.encoded("a"), "YQA=")

    def test_unknown_transports_refuse(self):
        with self.assertRaises(ValueError):
            tr.from_spec("telnet:x")


if __name__ == "__main__":
    unittest.main()
