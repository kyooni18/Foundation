import {describe,it,expect} from 'vitest';
import {lexicalCoverage,analyzeSearchQuery,requiresIdentityAnchor} from '../src/search.js';
describe('Unicode-aware retrieval terms',()=>{
 it('rejects Latin substring false positives',()=>{
  expect(lexicalCoverage(['pine'],'pipeline')).toBe(0);
  expect(lexicalCoverage(['comet'],'Commit cometary')).toBe(0);
  expect(lexicalCoverage(['ksl'],'KSL2')).toBe(0);
  expect(lexicalCoverage(['AppleDocker'],'AppleDockerExtra')).toBe(0);
 });
 it('matches casefolded camelCase identifiers and Korean particles',()=>{
  expect(lexicalCoverage(['appledocker'],'AppleDocker runs locally')).toBe(1);
  expect(lexicalCoverage(['docker'],'AppleDocker runs locally')).toBe(1);
  expect(lexicalCoverage(['컨테이너'],'컨테이너를 실행')).toBe(1);
  expect(lexicalCoverage(['제어'],'제어판')).toBe(0);
 });
 it('distinguishes entity lookups from natural-language paraphrases',()=>{
  for(const q of ['Comet','comet','Neptune','KSL','AppleDocker']) expect(requiresIdentityAnchor(analyzeSearchQuery(q))).toBe(true);
  for(const q of ['How are containers hosted on this Mac?','애플 실리콘에서 컨테이너를 실행하는 방법']) expect(requiresIdentityAnchor(analyzeSearchQuery(q))).toBe(false);
 });
});
