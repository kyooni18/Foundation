"""Version-39 responsibility-specific Source protocol; never approval/publication."""
import json
import focused_review as f
import guarded_haiku as g

def root(version):
    g.require(version in {'v39','v40','v41','v42'},'unknown faceted profile')
    return f.ROOT/('prompts/extraction-'+version)

def definitions(version='v39'):return json.loads((root(version)/'facets.json').read_text())
def stages(version='v39'):return {s['key']:s['stage'] for s in definitions(version)}
def spec(stage,version='v39'):
    rows=[s for s in definitions(version) if s['stage']==stage]
    g.require(len(rows)==1,'unknown review responsibility')
    return rows[0]
def prompt(stage,version='v39'):
    d=spec(stage,version)
    dims=json.dumps(d['dimensions'],ensure_ascii=False,separators=(',',':'))
    return (root(version)/'review.txt').read_text()+f'\nACTIVE responsibility: {stage}.\nACTIVE dimensions: {dims}.\nInventory responsibility: '+str(d['inventory']).lower()+'.\n'+d['question']

def schema(packet,stage,wire_revision='supported-v2',version='v39'):
    g.require(wire_revision in {'unsupported-v1','supported-v2'},'unknown faceted wire schema')
    d=spec(stage,version);source=packet['case']['source'];claims=packet['candidate']['claims'];n=len(claims)
    status={'type':'string','enum':['pass','reject','unresolved']}
    string={'type':'string'}
    if version=='v41':string['pattern']=r'[^ \t\r\n]'
    strings={'type':'array','items':string}
    evidence={'type':'array','minItems':1,'maxItems':1,'items':{'type':'object','additionalProperties':False,'properties':{'quote':{'type':'string','enum':[source]}},'required':['quote']}}
    dimensions={}
    for key in d['dimensions']:
        s=status
        if key=='korean_meaning' and not (f.has_korean(source) or any(c['lang']=='ko' for c in claims)):
            s={'type':'string','enum':[*status['enum'],'not_applicable']}
        dimensions[key]={'type':'object','additionalProperties':False,'properties':{'status':s,'reason':string},'required':['status','reason']}
    fields=dict(index={'type':'integer'},text=string,evidence=evidence,source_meaning=string,candidate_meaning=string,
        dimensions={'type':'object','additionalProperties':False,'properties':dimensions,'required':d['dimensions']},differences=strings,uncertainty_flags=strings)
    if version in {'v41','v42'}:fields['violations']=fields.pop('differences')
    if n:
        fields['index']={'type':'integer','enum':list(range(n))}
        fields['text']={'type':'string','enum':[c['text'] for c in claims]}
    inventory_fields=dict(proposition=string,evidence=evidence,claim_indices={'type':'array','items':{'type':'integer'}},status=status,reason=string)
    inventory={'type':'array','items':{'type':'object','additionalProperties':False,'properties':inventory_fields,'required':list(inventory_fields)}}
    if not d['inventory']:inventory['maxItems']=0
    props=dict(stage={'type':'string','enum':[stage]},verdict=status,uncertainty_flags=strings,
        claims={'type':'array','minItems':n,'maxItems':n,'items':{'type':'object','additionalProperties':False,'properties':fields,'required':list(fields)}},inventory=inventory)
    result={'type':'object','additionalProperties':False,'properties':props,'required':list(props)}
    if wire_revision=='supported-v2':
        def portable(value):
            if isinstance(value,dict):
                value.pop('maxItems',None)
                if 'minItems' in value:value['minItems']=min(value['minItems'],1)
                for child in value.values():portable(child)
            elif isinstance(value,list):
                for child in value:portable(child)
        portable(result)
    return result

def body(packet,stage,model,wire_revision='supported-v2',version='v39'):
    payload=dict(original_source=packet['case']['source'],candidate_batch=packet['candidate'],authored_at=packet['case'].get('authored_at'),candidate_count=len(packet['candidate']['claims']),required_checked_indices=list(range(len(packet['candidate']['claims']))))
    return dict(model=model,max_tokens=6000,system=prompt(stage,version),messages=[dict(role='user',content=f.canonical(payload).decode())],output_config=dict(effort='low',format=dict(type='json_schema',schema=schema(packet,stage,wire_revision,version))),service_tier='standard_only')

def check(j,packet,stage,shape,version='v39'):
    g.require(stage is not None,'review responsibility binding required');d=spec(stage,version);shape(j,schema(packet,stage,version=version))
    violation_key='violations' if version in {'v41','v42'} else 'differences'
    source=packet['case']['source'];claims=packet['candidate']['claims'];g.require(j['stage']==stage,'review responsibility changed')
    g.require(all(s.strip() for s in j['uncertainty_flags']),'empty uncertainty flag')
    unresolved=bool(j['uncertainty_flags']);rejected=False
    g.require(len(j['claims'])==len(claims),'review omitted Claim')
    for i,(row,claim) in enumerate(zip(j['claims'],claims)):
        g.require(type(row['index']) is int and row['index']==i and row['text']==claim['text'],'changed Claim identity')
        g.require(row['evidence']==[dict(quote=source)],'complete original context required')
        g.require(row['source_meaning'].strip() and row['candidate_meaning'].strip(),'empty reconstruction')
        failure=False
        for key in d['dimensions']:
            dim=row['dimensions'][key];g.require(dim['reason'].strip(),'blank dimension reason');s=dim['status']
            g.require(s!='not_applicable' or not (claim['lang']=='ko' or f.has_korean(source)),'unassessed Korean dependency')
            rejected |= s=='reject';unresolved |= s=='unresolved';failure |= s in {'reject','unresolved'}
        g.require(all(s.strip() for s in row[violation_key]+row['uncertainty_flags']),'blank difference/uncertainty')
        g.require(failure or not (row[violation_key] or row['uncertainty_flags']),'passing responsibility contradicts reported differences')
        unresolved |= bool(row['uncertainty_flags'])
    inventory=j['inventory'];g.require(d['inventory'] or not inventory,'inventory is outside this responsibility')
    g.require(not d['inventory'] or inventory or (not claims and isinstance(packet['candidate'].get('empty_reason'),str) and packet['candidate']['empty_reason'].strip()),'empty inventory requires valid empty extraction')
    seen=set();mapped=set();bijective=True
    for row in inventory:
        p=row['proposition'];start=source.find(p)
        g.require(p.strip() and start>=0 and source.find(p,start+1)<0 and row['reason'].strip(),'canonical original inventory required')
        g.require(p not in seen,'duplicate source inventory entry');seen.add(p)
        g.require(row['evidence']==[dict(quote=source)],'complete original context required')
        indices=row['claim_indices'];g.require(all(type(i) is int and 0<=i<len(claims) for i in indices) and len(indices)==len(set(indices)),'invalid inventory Claim index')
        bijective &= len(indices)==1 and not mapped.intersection(indices);mapped.update(indices)
        g.require(row['status']!='pass' or indices,'passing inventory has no Claim')
        rejected |= row['status']=='reject';unresolved |= row['status']=='unresolved'
    expected='reject' if rejected else 'unresolved' if unresolved else 'pass'
    g.require(j['verdict']==expected,'contradictory aggregate verdict')
    g.require(expected!='pass' or not d['inventory'] or (bijective and len(mapped)==len(claims) and len(inventory)==len(claims)),'passing inventory must map propositions and Claims one-to-one')
    return dict(protocol_valid=True,verdict=j['verdict'],claim_count=len(claims),inventory_count=len(inventory),semantic_acceptance=None,production_approval=False)
