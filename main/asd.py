import psycopg
from pgvector.psycopg import register_vector

DSN = "postgresql://foundation:host@localhost:5432/foundation_db1"  # 상황에 맞게 localhost/db

with psycopg.connect(DSN) as conn:
    register_vector(conn)

    # pgvector 확장 확인
    conn.execute("CREATE EXTENSION IF NOT EXISTS vector")

    # 간단 확인: vector 타입 캐스팅이 되는지
    x = conn.execute("SELECT '[1,2,3]'::vector").fetchone()[0]
    print("vector ok:", x)