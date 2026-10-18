/** Reproducible labeled end-to-end retrieval evaluation on a disposable database only.
 * TEST_DATABASE_URL=... EMBEDDING_PROVIDER=gemini GEMINI_API_KEY=... npx tsx bench/retrieval-quality.ts
 * Optional: QUALITY_SEARCH_MODULE=../src/.search-baseline.js (export an old search.ts there).
 * QUALITY_PHASE=before emulates v7 scopes; default runs the v8 migration before evaluation.
 * QUALITY_EMBED_CACHE caches vendor vectors for paired, deterministic before/after runs.
 * Labels include deliberately weak topical decoys. Precision@5 uses five slots (empty slots count 0),
 * returnedPrecision and FPR distinguish abstention from irrelevant results. Recall uses complete fixture qrels.
 */
import 'dotenv/config';
import {readFileSync,writeFileSync,existsSync,mkdirSync} from 'node:fs';
import {createAIProvider} from '../src/ai.js';
import {loadConfig} from '../src/config.js';
import {createTestDb} from '../tests/helpers/db.js';
import {contentHash} from '../src/utils.js';
import * as current from '../src/search.js';
const fixture=JSON.parse(readFileSync(new URL('./retrieval-quality.json',import.meta.url),'utf8'));
const search:typeof current=process.env.QUALITY_SEARCH_MODULE ? await import(process.env.QUALITY_SEARCH_MODULE) : current;
const ai=createAIProvider(loadConfig());
const cachePath=process.env.QUALITY_EMBED_CACHE || 'bench/results/quality-embeddings.json';
mkdirSync('bench/results',{recursive:true});
const cache=existsSync(cachePath)?JSON.parse(readFileSync(cachePath,'utf8')):{};
const embed=async (text:string,purpose:'query'|'document'='document')=>{
 const key=JSON.stringify([ai.identity(),purpose,text]);
 if(!(key in cache)){cache[key]=await ai.embed(text,purpose);writeFileSync(cachePath,JSON.stringify(cache));}
 return cache[key];
};
const cached=Object.create(ai);cached.embed=embed;
const t=await createTestDb();
try {
 const ids=new Map<string,string>();
 for(const doc of fixture.documents){
  const embedding=await embed(doc.text);
  const row=await t.db.query(`INSERT INTO atoms(id,content,normalized_content,content_hash,type,tags,metadata,scope_kind,scope_key,status,valid_from,expires_at,
   vector,vector_provider,vector_model,vector_dimensions,vector_embedded_at)
   VALUES($14,$1,lower($1),$2,'imported',$3,$4,'project',$5,$6,$7,$8,$9::real[]::halfvec,$10,$11,$12,$13) RETURNING id`,
   [doc.text,contentHash(doc.text),doc.tags||[],{project:doc.project},doc.corrupted||doc.project.toLowerCase(),doc.status||'active',doc.validFrom||null,doc.expires||null,
   embedding?.vector||null,embedding?.provider||null,embedding?.model||null,embedding?.dimensions||null,embedding?new Date():null,contentHash(doc.key).slice(0,32).replace(/(.{8})(.{4})(.{4})(.{4})(.{12})/, '$1-$2-$3-$4-$5')]);
  ids.set(doc.key,row.rows[0]!.id);
 }
 await t.db.query('INSERT INTO foundation_schema(version) VALUES (7) ON CONFLICT DO NOTHING');
 await t.db.query('DELETE FROM foundation_schema WHERE version=8');
 if(process.env.QUALITY_PHASE!=='before')await t.db.initialize();
 const reverse=new Map([...ids].map(([k,v])=>[v,k]));
 const results=[];
 for(const q of fixture.queries){
  const trace=search.newSearchTrace(true);
  const items=await search.searchFoundation(t.db,cached,q.query,{limit:5,project:q.project,asOf:q.asOf,history:q.history,trace});
  const returned=items.map(i=>reverse.get(i.id)!);const relevant=new Set<string>(q.relevant);
  const hits=returned.map(k=>relevant.has(k)?1:0);const tp=hits.reduce((a:number,b:number)=>a+b,0);
  const dcg=hits.reduce((s:number,h:number,i:number)=>s+h/Math.log2(i+2),0);
  const idcg=Array.from({length:Math.min(5,relevant.size)},(_,i)=>1/Math.log2(i+2)).reduce((a,b)=>a+b,0);
  results.push({query:q.query,split:q.split,project:q.project,asOf:q.asOf,history:q.history,relevant:q.relevant,returned,
   precisionAt5:tp/5,recallAt5:relevant.size?tp/relevant.size:null,mrr:hits.indexOf(1)<0?0:1/(hits.indexOf(1)+1),ndcg:idcg?dcg/idcg:null,
   falsePositives:returned.length-tp,truePositives:tp,noAnswerCorrect:relevant.size?null:returned.length===0,
   ms:trace.totalMs!,generators:trace.generators,candidates:trace.candidates.map(c=>({...c,key:reverse.get(c.key.split(':')[1]!)||c.key}))});
 }
 const avg=(a:number[])=>a.reduce((s,v)=>s+v,0)/Math.max(1,a.length);
 const summary=(rows:typeof results)=>{
  const pos=rows.filter(r=>r.relevant.length);const neg=rows.filter(r=>!r.relevant.length);
  const tp=rows.reduce((s,r)=>s+r.truePositives,0),fp=rows.reduce((s,r)=>s+r.falsePositives,0);
  const lat=rows.map(r=>r.ms).sort((a,b)=>a-b);
  return {queries:rows.length,precisionAt5:avg(pos.map(r=>r.precisionAt5)),recallAt5:avg(pos.map(r=>r.recallAt5!)),mrr:avg(pos.map(r=>r.mrr)),ndcg:avg(pos.map(r=>r.ndcg!)),
   returnedPrecision:tp/Math.max(1,tp+fp),falsePositiveRate:fp/Math.max(1,tp+fp),noAnswerAccuracy:avg(neg.map(r=>r.noAnswerCorrect?1:0)),
   meanMs:avg(lat),p50Ms:lat[Math.floor(lat.length*.5)],p95Ms:lat[Math.min(lat.length-1,Math.floor(lat.length*.95))]};
 };
 const report={provider:ai.identity(),phase:process.env.QUALITY_PHASE||'after',summary:summary(results),test:summary(results.filter(r=>r.split==='test')),results};
 const path=`bench/results/quality-${process.env.QUALITY_PHASE||'after'}.json`;writeFileSync(path,JSON.stringify(report,null,2));console.log(JSON.stringify({path,summary:report.summary,test:report.test},null,2));
}finally{await t.drop();}
