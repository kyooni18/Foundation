-- INERT PROPOSAL. Apply only after explicit user authorization for $0.05 more.
-- Guards require this exact settled checkpoint; later charges require re-audit.
BEGIN IMMEDIATE;
CREATE TEMP TABLE policy_precondition(ok INTEGER NOT NULL CHECK(ok=1));
INSERT INTO policy_precondition SELECT CASE WHEN
 (SELECT COUNT(*) FROM budget)=1 AND
 (SELECT ceiling FROM budget WHERE id=1)=4882653350 AND
 (SELECT prior FROM budget WHERE id=1)=4782653350 AND
 (SELECT COUNT(*) FROM request)=40 AND
 (SELECT SUM(COALESCE(actual,estimate)) FROM request)=97054700 AND
 (SELECT COUNT(*) FROM request WHERE actual IS NULL OR actual>estimate)=0 AND
 4932653350<5000000000
 THEN 1 ELSE 0 END;
UPDATE budget SET ceiling=4932653350 WHERE id=1;
DROP TABLE policy_precondition;
COMMIT;
