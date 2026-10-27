"""The judge and the privacy gate, on made-up runs (rule #5, the logic level).

    python3 -m unittest rig/oneclick/test_rows.py
"""

import json
import os
import shutil
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import rows  # noqa: E402


def made_up_run(tmp, transport="ssh:aspire", notes="Windows started first with no key", job=None, secrets=None, facts=True):
    d = os.path.join(tmp, "aspire-20261028T100000Z")
    os.makedirs(os.path.join(d, "stages", "01-rollback"))
    with open(os.path.join(d, "run.json"), "w") as f:
        json.dump({"manager": "0.1.0", "transport": transport, "plan": {"stages": [{"stage": "rollback"}]}}, f)
    with open(os.path.join(d, "secrets.json"), "w") as f:
        json.dump(secrets or {"password": "pw-made-up-123", "walkaway-account": "Pat"}, f)
    if facts:
        with open(os.path.join(d, "facts.json"), "w") as f:
            json.dump({"manufacturer": "Acer", "model": "Aspire A515-51G", "bios_vendor": "Insyde Corp.", "bios_version": "V1.21", "os_build": "26200"}, f)
    if job is not None:
        with open(os.path.join(d, "stages", "01-rollback", "job.json"), "w") as f:
            json.dump(job, f)
    with open(os.path.join(d, "stages", "01-rollback", "SHA256SUMS"), "w") as f:
        f.write("ab  ./rollback.json\n")
    with open(os.path.join(d, "journal.jsonl"), "w") as f:
        for e in ({"event": "stage-start", "index": 0, "stage": "rollback", "folder": "01-rollback", "utc": "2026-10-28T10:00:00Z"},
                  {"event": "stage-done", "index": 0, "stage": "rollback", "word": "rolled-back", "expected": True, "notes": notes, "utc": "2026-10-28T10:10:00Z"}):
            f.write(json.dumps(e) + "\n")
    return d


class RowTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp()
        self.csv = os.path.join(self.tmp, "oneclick-runs.csv")
        self.said = []

    def tearDown(self):
        shutil.rmtree(self.tmp)

    def out(self, *a, **k):
        self.said.append(" ".join(str(x) for x in a))

    def read(self):
        with open(self.csv) as f:
            return f.read()

    def test_a_physical_row_names_its_machine_from_the_facts_and_passes_as_pass(self):
        d = made_up_run(self.tmp)
        self.assertEqual(rows.promote(d, self.csv, out=self.out), 0)
        text = self.read()
        self.assertIn('"physical","Acer Aspire A515-51G","Insyde Corp. V1.21","26200"', text)
        self.assertIn('"rollback","rolled-back","pass"', text)
        self.assertEqual(text.splitlines()[0], ",".join(f'"{h}"' for h in rows.HEADER))

    def test_the_rig_never_writes_pass_only_pass_plumbing_and_a_surprise_is_fail(self):
        self.assertEqual(rows.class_of("rig", True), "pass-plumbing")
        self.assertEqual(rows.class_of("physical", True), "pass")
        self.assertEqual(rows.class_of("physical", False), "fail")
        self.assertEqual(rows.class_of("rig", False), "fail")
        self.assertEqual(rows.class_of("rig", True, "rig-standin-fresh-windows"), "stand-in", "a stand-in is never a pass of any kind")

    def test_known_personal_values_are_replaced_by_placeholders(self):
        job = {"intent": {"account": {"windows_name": "Pat", "linux_name": "pat"}},
               "harvest": {"wifi": {"profiles": [{"name": "Home Net 5G", "ssid": "Home Net 5G"}]}},
               "identity": {"system_disk": {"serial_number": "SN123456789", "unique_id": "eui.0011"}}}
        d = made_up_run(self.tmp, notes="user Pat on Home Net 5G, disk SN123456789, pw pw-made-up-123", job=job)
        self.assertEqual(rows.promote(d, self.csv, out=self.out), 0)
        text = self.read()
        for bad in ("Pat", "Home Net 5G", "SN123456789", "pw-made-up-123"):
            self.assertNotIn(bad, text)
        self.assertIn("user <name> on <wifi>, disk <serial>, pw <password>", text)

    def test_a_value_that_survives_scrubbing_is_refused_never_written(self):
        d = made_up_run(self.tmp)
        values = [("Pat", "<name>")]
        self.assertEqual(rows.leaks("hello pat", values), ["<name>"])
        self.assertEqual(rows.leaks(rows.scrub("hello pat", values), values), [])

    def test_the_computer_name_is_scrubbed(self):
        os.environ["ONECLICK_HOSTNAME"] = "pats-laptop"
        try:
            d = made_up_run(self.tmp, notes="pulled from pats-laptop")
            self.assertEqual(rows.promote(d, self.csv, out=self.out), 0)
            self.assertIn("pulled from <computer>", self.read())
        finally:
            del os.environ["ONECLICK_HOSTNAME"]

    def test_the_bench_name_and_the_judge_s_own_columns_are_never_rewritten(self):
        d = made_up_run(self.tmp, transport="hyperv:UPGRIGHV", notes="the rig cannot hold it", secrets={"password": "pw-made-up-123", "walkaway-account": "rig"})
        self.assertEqual(rows.promote(d, self.csv, out=self.out), 0)
        text = self.read()
        self.assertIn('"rig","Acer Aspire', text)
        self.assertIn("the rig cannot hold it", text)

    def test_markup_in_a_note_is_dropped(self):
        self.assertEqual(rows.clean_notes("F:\\x.json: #< CLIXML\n<Objs><S>noise</S></Objs>"), "F:\\x.json:")

    def test_a_run_is_promoted_once(self):
        d = made_up_run(self.tmp)
        self.assertEqual(rows.promote(d, self.csv, out=self.out), 0)
        self.assertEqual(rows.promote(d, self.csv, out=self.out), 2)
        self.assertEqual(len(self.read().splitlines()), 2, "the header and one row, not two")

    def test_a_csv_with_another_header_is_refused(self):
        with open(self.csv, "w") as f:
            f.write('"timestamp","something else"\n')
        d = made_up_run(self.tmp)
        self.assertEqual(rows.promote(d, self.csv, out=self.out), 2)

    def test_a_dry_run_writes_nothing(self):
        d = made_up_run(self.tmp)
        self.assertEqual(rows.promote(d, self.csv, dry_run=True, out=self.out), 0)
        self.assertFalse(os.path.exists(self.csv))
        self.assertFalse(os.path.exists(os.path.join(d, ".promoted")))

    def test_a_run_without_facts_says_so_and_borrowed_facts_are_named(self):
        d = made_up_run(self.tmp, facts=False)
        self.assertEqual(rows.promote(d, self.csv, dry_run=True, out=self.out), 0)
        self.assertIn("unknown (no facts recorded)", "\n".join(self.said))
        other = os.path.join(self.tmp, "other-run")
        os.makedirs(other)
        with open(os.path.join(other, "facts.json"), "w") as f:
            json.dump({"manufacturer": "Microsoft Corporation", "model": "Virtual Machine"}, f)
        self.said = []
        rows.promote(d, self.csv, facts_path=os.path.join(other, "facts.json"), dry_run=True, out=self.out)
        self.assertIn("facts from other-run", "\n".join(self.said))

    def test_each_row_points_at_its_evidence_by_hash(self):
        d = made_up_run(self.tmp)
        rows.promote(d, self.csv, out=self.out)
        import hashlib
        want = hashlib.sha256(b"ab  ./rollback.json\n").hexdigest()
        self.assertIn(want, self.read())


if __name__ == "__main__":
    unittest.main()
