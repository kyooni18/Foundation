-- INERT: requires new approval for the final $0.01734664 capacity below $5.
BEGIN IMMEDIATE;
CREATE TEMP TABLE policy_precondition(ok INTEGER NOT NULL CHECK(ok=1));
INSERT INTO policy_precondition SELECT CASE WHEN
 (SELECT COUNT(*) FROM budget)=1 AND
 (SELECT ceiling FROM budget WHERE id=1)=4982653350 AND
 (SELECT prior FROM budget WHERE id=1)=4782653350 AND
 (SELECT COUNT(*) FROM request)=108 AND
 (SELECT SUM(COALESCE(actual,estimate)) FROM request)=196172700 AND
 (SELECT COUNT(*) FROM request WHERE actual IS NULL OR actual>estimate)=0 AND
 4999999990<5000000000
 THEN 1 ELSE 0 END;
UPDATE budget SET ceiling=4999999990 WHERE id=1;
DROP TABLE policy_precondition;
COMMIT;
