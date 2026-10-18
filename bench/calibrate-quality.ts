/** Offline semantic-only gate sweep over *calibration* qrels, never the held-out queries.
 * First run retrieval-quality.ts. This evaluates acceptance, not complete end-to-end ranking;
 * the latter is measured by retrieval-quality.ts. No provider calls or database writes.
 */
import {readFileSync} from 'node:fs';
import {analyzeSearchQuery,fuseSearchCandidates,requiresIdentityAnchor} from '../src/search.js';
const report=JSON.parse(readFileSync(process.argv[2]||'bench/results/quality-after.json','utf8'));
const cases=report.results.filter((r:any)=>r.split==='calibration');
const rows=[];
for(const threshold of [.6,.64,.66,.68,.7,.72,.75]){
 let tp=0,fp=0,fn=0;
 for(const q of cases){
  const analysis=analyzeSearchQuery(q.query);
  const fused=fuseSearchCandidates(q.candidates.map((c:any)=>({key:c.key,...c.signals})),{
   queryTokenCount:analysis.tokens.length,requireAnchor:requiresIdentityAnchor(analysis),semanticOnlyMin:threshold});
  const accepted=new Set(fused.filter(c=>c.eligible&&c.score>=.22).slice(0,5).map(c=>c.key));
  tp+=q.relevant.filter((key:string)=>accepted.has(key)).length;
  fn+=q.relevant.filter((key:string)=>!accepted.has(key)).length;
  fp+=[...accepted].filter(key=>!q.relevant.includes(key)).length;
 }
 rows.push({semanticOnlyMin:threshold,tp,fp,fn,precision:tp/Math.max(1,tp+fp),recall:tp/Math.max(1,tp+fn),f1:2*tp/Math.max(1,2*tp+fp+fn)});
}
console.log(JSON.stringify({cases:cases.length,rows},null,2));
