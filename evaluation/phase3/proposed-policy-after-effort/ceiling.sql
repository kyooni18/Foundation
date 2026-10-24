-- INERT: requires new human approval for an additional $0.05 beyond the prior grant.
BEGIN IMMEDIATE;
CREATE TEMP TABLE policy_precondition(ok INTEGER NOT NULL CHECK(ok=1));
INSERT INTO policy_precondition SELECT CASE WHEN
 (SELECT COUNT(*) FROM budget)=1 AND
 (SELECT ceiling FROM budget WHERE id=1)=4932653350 AND
 (SELECT prior FROM budget WHERE id=1)=4782653350 AND
 (SELECT COUNT(*) FROM request)=76 AND
 (SELECT SUM(COALESCE(actual,estimate)) FROM request)=146170200 AND
 (SELECT COUNT(*) FROM request WHERE actual IS NULL OR actual>estimate)=0 AND
 4982653350<5000000000
 THEN 1 ELSE 0 END;
UPDATE budget SET ceiling=4982653350 WHERE id=1;
DROP TABLE policy_precondition;
COMMIT;
