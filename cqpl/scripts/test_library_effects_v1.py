#!/usr/bin/env python3
import unittest,sys
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parent))
from library_effects_v1 import *

def reg(rid,sums,status="scaffold"):
    return parse_registry_data({"schema_version":"library_effect_v1","capability":"library_effects_v1","registry_id":rid,"status":status,"summaries":sums})
def s(sid,matcher,effect="alloc"):
    return {"summary_id":sid,"language":"rust","matcher":matcher,"preconditions":[],"normal_return_effects":[{"kind":effect}],"unwind_effects":[],"certainty":"may_abstract","provenance":{"basis":"test","source":"synthetic"}}
def ps(sid,key):
    x=s(sid,{"kind":"producer_evidence_kind","value":key},"ownership_transfer")
    x["legacy_projection"]={"allocation_disposition_kind":key,"obligation_effect":"e","basis":"b","target_variable":"none"}
    return x

class T(unittest.TestCase):
    def test_unknown_effect(self):
        x=s("bad",{"kind":"exact_def_path","value":"x"}); x["normal_return_effects"]=[{"kind":"bad"}]
        with self.assertRaises(RegistryValidationError): reg("r",[x])
    def test_unknown_key(self):
        x=s("bad",{"kind":"exact_def_path","value":"x"}); x["heuristic"]=1
        with self.assertRaises(RegistryValidationError): reg("r",[x])
    def test_dup_summary(self):
        with self.assertRaises(RegistryValidationError): validate_registry_set([reg("a",[s("same",{"kind":"exact_def_path","value":"a"})]),reg("b",[s("same",{"kind":"exact_def_path","value":"b"})])])
    def test_priority(self):
        regs=validate_registry_set([reg("r",[s("suffix",{"kind":"def_path_suffix","value":"Box::into_raw"}),s("exact",{"kind":"exact_def_path","value":"alloc::boxed::Box::into_raw"})])])
        self.assertEqual(lookup_summary(regs,FunctionIdentity("rust",def_path="alloc::boxed::Box::into_raw")).summary_id,"exact")
    def test_ambiguity(self):
        regs=validate_registry_set([reg("r",[s("a",{"kind":"def_path_suffix","value":"Box::into_raw"}),s("b",{"kind":"def_path_suffix","value":"boxed::Box::into_raw"})])])
        with self.assertRaises(LookupAmbiguityError): lookup_summary(regs,FunctionIdentity("rust",def_path="alloc::boxed::Box::into_raw"))
    def test_none(self):
        self.assertIsNone(lookup_summary(validate_registry_set([reg("r",[])]),FunctionIdentity("rust",def_path="x")))
    def test_producer(self):
        hit=lookup_summary(validate_registry_set([reg("r",[ps("into","box_into_raw")],"candidate")]),FunctionIdentity("rust",producer_evidence_kind="box_into_raw"))
        self.assertEqual(hit.legacy_projection.basis,"b")
    def test_producer_requires_projection(self):
        with self.assertRaises(RegistryValidationError): reg("r",[s("bad",{"kind":"producer_evidence_kind","value":"x"})],"candidate")
    def test_projection_nonproducer_rejected(self):
        x=s("bad",{"kind":"exact_def_path","value":"x"}); x["legacy_projection"]={"allocation_disposition_kind":"x","obligation_effect":"x","basis":"x","target_variable":"none"}
        with self.assertRaises(RegistryValidationError): reg("r",[x])

if __name__=="__main__":
    result=unittest.TextTestRunner(verbosity=2).run(unittest.defaultTestLoader.loadTestsFromTestCase(T))
    if result.wasSuccessful(): print("LIBRARY_EFFECTS_V1_UNIT: PASS")
    raise SystemExit(0 if result.wasSuccessful() else 1)
