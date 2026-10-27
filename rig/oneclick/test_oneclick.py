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
        d = make_run(self.tmp, [{"stage": "look", "expect": ["YELLOW", "RED"]}, {"stage": "keep-windows"}])
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
