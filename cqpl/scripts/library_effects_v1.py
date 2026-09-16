#!/usr/bin/env python3
from __future__ import annotations
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable, Optional
import json

SCHEMA_VERSION="library_effect_v1"
CAPABILITY="library_effects_v1"
CERTAINTIES={"may_abstract"}
LANGUAGES={"rust","c"}
MATCH_PRIORITIES={"producer_evidence_kind":400,"exact_def_path":300,"def_path_suffix":200,"symbol_exact":100}
EFFECT_KINDS={"alloc","dealloc","realloc","ownership_transfer","ownership_reclaim","escape","forget","read","write","initialize","invalidate"}
TOP_KEYS={"schema_version","capability","registry_id","status","summaries"}
SUMMARY_REQUIRED={"summary_id","language","matcher","preconditions","normal_return_effects","unwind_effects","certainty","provenance"}
SUMMARY_KEYS=SUMMARY_REQUIRED|{"legacy_projection"}
MATCHER_KEYS={"kind","value"}
EFFECT_KEYS={"kind","allocation","from","to","allocator_family","note"}
PROVENANCE_KEYS={"basis","source"}
PROJECTION_KEYS={"allocation_disposition_kind","obligation_effect","basis","target_variable"}
STATUSES={"scaffold","candidate","validated"}
TARGET_VARIABLES={"return","none"}

class LibraryEffectsError(ValueError): pass
class RegistryValidationError(LibraryEffectsError): pass
class LookupAmbiguityError(LibraryEffectsError): pass

@dataclass(frozen=True)
class FunctionIdentity:
    language:str
    def_path:Optional[str]=None
    symbol:Optional[str]=None
    producer_evidence_kind:Optional[str]=None

@dataclass(frozen=True)
class LegacyProjection:
    allocation_disposition_kind:str
    obligation_effect:str
    basis:str
    target_variable:str

@dataclass(frozen=True)
class Summary:
    summary_id:str
    language:str
    matcher_kind:str
    matcher_value:str
    preconditions:tuple[dict[str,Any],...]
    normal_return_effects:tuple[dict[str,Any],...]
    unwind_effects:tuple[dict[str,Any],...]
    certainty:str
    provenance:dict[str,str]
    registry_id:str
    legacy_projection:Optional[LegacyProjection]
    @property
    def priority(self)->int: return MATCH_PRIORITIES[self.matcher_kind]

@dataclass(frozen=True)
class Registry:
    registry_id:str
    status:str
    summaries:tuple[Summary,...]
    source_path:Path

def _fail(message:str)->None: raise RegistryValidationError(message)
def _require_exact_keys(obj,allowed,where):
    unknown=set(obj)-allowed
    if unknown: _fail(f"{where}: unknown keys: {sorted(unknown)}")
def _nonempty_string(value,where):
    if not isinstance(value,str) or not value.strip(): _fail(f"{where}: expected non-empty string")
    return value
def _validate_effect(effect,where):
    if not isinstance(effect,dict): _fail(f"{where}: effect must be object")
    _require_exact_keys(effect,EFFECT_KEYS,where)
    kind=effect.get("kind")
    if kind not in EFFECT_KINDS: _fail(f"{where}: unsupported effect kind: {kind!r}")
    for key in ("allocation","from","to","allocator_family"):
        if key in effect: _nonempty_string(effect[key],f"{where}.{key}")
    if "note" in effect and not isinstance(effect["note"],str): _fail(f"{where}.note: expected string")
    return dict(effect)
def _validate_matcher(matcher,where):
    if not isinstance(matcher,dict): _fail(f"{where}: matcher must be object")
    _require_exact_keys(matcher,MATCHER_KEYS,where)
    kind=matcher.get("kind")
    if kind not in MATCH_PRIORITIES: _fail(f"{where}: unsupported matcher kind: {kind!r}")
    return kind,_nonempty_string(matcher.get("value"),f"{where}.value")
def _validate_provenance(value,where):
    if not isinstance(value,dict): _fail(f"{where}: provenance must be object")
    _require_exact_keys(value,PROVENANCE_KEYS,where)
    return {"basis":_nonempty_string(value.get("basis"),f"{where}.basis"),
            "source":_nonempty_string(value.get("source"),f"{where}.source")}
def _validate_projection(value,where):
    if not isinstance(value,dict): _fail(f"{where}: legacy_projection must be object")
    _require_exact_keys(value,PROJECTION_KEYS,where)
    missing=PROJECTION_KEYS-set(value)
    if missing: _fail(f"{where}: missing keys: {sorted(missing)}")
    target=value["target_variable"]
    if target not in TARGET_VARIABLES: _fail(f"{where}.target_variable: invalid value {target!r}")
    return LegacyProjection(_nonempty_string(value["allocation_disposition_kind"],f"{where}.allocation_disposition_kind"),
                            _nonempty_string(value["obligation_effect"],f"{where}.obligation_effect"),
                            _nonempty_string(value["basis"],f"{where}.basis"),target)

def parse_registry_data(data,source_path="<memory>"):
    source_path=Path(source_path)
    if not isinstance(data,dict): _fail(f"{source_path}: registry must be object")
    _require_exact_keys(data,TOP_KEYS,str(source_path))
    if data.get("schema_version")!=SCHEMA_VERSION: _fail(f"{source_path}: schema_version must be {SCHEMA_VERSION!r}")
    if data.get("capability")!=CAPABILITY: _fail(f"{source_path}: capability must be {CAPABILITY!r}")
    registry_id=_nonempty_string(data.get("registry_id"),f"{source_path}.registry_id")
    status=data.get("status")
    if status not in STATUSES: _fail(f"{source_path}: invalid status {status!r}")
    raw=data.get("summaries")
    if not isinstance(raw,list): _fail(f"{source_path}.summaries: expected list")
    seen=set(); summaries=[]
    for index,item in enumerate(raw):
        where=f"{source_path}.summaries[{index}]"
        if not isinstance(item,dict): _fail(f"{where}: summary must be object")
        _require_exact_keys(item,SUMMARY_KEYS,where)
        missing=SUMMARY_REQUIRED-set(item)
        if missing: _fail(f"{where}: missing keys: {sorted(missing)}")
        sid=_nonempty_string(item["summary_id"],f"{where}.summary_id")
        if sid in seen: _fail(f"{where}: duplicate summary_id {sid!r}")
        seen.add(sid)
        language=item["language"]
        if language not in LANGUAGES: _fail(f"{where}: unsupported language {language!r}")
        mk,mv=_validate_matcher(item["matcher"],f"{where}.matcher")
        pre=item["preconditions"]
        if not isinstance(pre,list) or any(not isinstance(x,dict) for x in pre): _fail(f"{where}.preconditions: expected list of objects")
        normal=item["normal_return_effects"]; unwind=item["unwind_effects"]
        if not isinstance(normal,list): _fail(f"{where}.normal_return_effects: expected list")
        if not isinstance(unwind,list): _fail(f"{where}.unwind_effects: expected list")
        certainty=item["certainty"]
        if certainty not in CERTAINTIES: _fail(f"{where}: unsupported certainty {certainty!r}")
        projection=_validate_projection(item["legacy_projection"],f"{where}.legacy_projection") if "legacy_projection" in item else None
        if mk=="producer_evidence_kind" and projection is None: _fail(f"{where}: producer_evidence_kind requires legacy_projection")
        if mk!="producer_evidence_kind" and projection is not None: _fail(f"{where}: legacy_projection requires producer_evidence_kind")
        summaries.append(Summary(sid,language,mk,mv,tuple(dict(x) for x in pre),
                                 tuple(_validate_effect(x,f"{where}.normal_return_effects[{i}]") for i,x in enumerate(normal)),
                                 tuple(_validate_effect(x,f"{where}.unwind_effects[{i}]") for i,x in enumerate(unwind)),
                                 certainty,_validate_provenance(item["provenance"],f"{where}.provenance"),
                                 registry_id,projection))
    return Registry(registry_id,status,tuple(summaries),source_path)

def load_registry(path):
    path=Path(path)
    try: data=json.loads(path.read_text(encoding="utf-8"))
    except (OSError,json.JSONDecodeError) as exc: raise RegistryValidationError(f"{path}: cannot load JSON: {exc}") from exc
    return parse_registry_data(data,path)

def validate_registry_set(registries):
    registries=tuple(registries); ids={}; sids={}
    for reg in registries:
        if reg.registry_id in ids: _fail(f"duplicate registry_id {reg.registry_id!r}")
        ids[reg.registry_id]=str(reg.source_path)
        for s in reg.summaries:
            if s.summary_id in sids: _fail(f"duplicate summary_id {s.summary_id!r} across registries")
            sids[s.summary_id]=str(reg.source_path)
    return registries

def _matches(s,i):
    if s.language!=i.language: return False
    if s.matcher_kind=="producer_evidence_kind": return i.producer_evidence_kind==s.matcher_value
    if s.matcher_kind=="exact_def_path": return i.def_path==s.matcher_value
    if s.matcher_kind=="def_path_suffix": return i.def_path is not None and i.def_path.endswith(s.matcher_value)
    if s.matcher_kind=="symbol_exact": return i.symbol==s.matcher_value
    raise AssertionError(s.matcher_kind)

def lookup_summary(registries,identity):
    if identity.language not in LANGUAGES: raise LibraryEffectsError(f"unsupported lookup language: {identity.language!r}")
    matches=[s for r in registries for s in r.summaries if _matches(s,identity)]
    if not matches: return None
    p=max(s.priority for s in matches)
    best=sorted((s for s in matches if s.priority==p),key=lambda s:(s.summary_id,s.registry_id))
    if len(best)!=1: raise LookupAmbiguityError("ambiguous library summary lookup at equal priority")
    return best[0]
