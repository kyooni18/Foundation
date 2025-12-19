import os

from fastapi import FastAPI, HTTPException
from contextlib import asynccontextmanager
from pydantic import BaseModel, Field
import psycopg
from psycopg_pool import ConnectionPool
from pgvector.psycopg import register_vector
import Embeddings

# 컨테이너/로컬 어디서든 동일하게 동작하도록 환경변수 우선, 없으면 로컬 기본값을 사용합니다.
DSN = os.getenv("DATABASE_URL") or "postgresql://foundation:host@localhost:5432/foundation_db1"

# 새 커넥션이 만들어질 때 pgvector 타입을 등록 (user snippet 방식)
def _configure_conn(conn: psycopg.Connection) -> None:
    register_vector(conn)

# NOTE: 아래 테이블은 DB에 미리 만들어 두셔야 합니다.
# 예시:
#   CREATE TABLE IF NOT EXISTS embeddings_store (
#     id BIGSERIAL PRIMARY KEY,
#     text TEXT NOT NULL,
#     embedding VECTOR(<EMBEDDING_DIM>) NOT NULL,
#     created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
#   );
# <EMBEDDING_DIM>은 embeddings.embed()가 반환하는 벡터 차원으로 바꾸세요.
TABLE_NAME = os.getenv("EMBEDDINGS_TABLE", "primary_db")
EMBEDDING_DIM = os.getenv("EMBEDDING_DIM", "1024")
INSERT_SQL = f"INSERT INTO {TABLE_NAME} (text, embedding, metadata) VALUES (%s, %s, %s)"


class ShootRequest_Payload(BaseModel):
    text: str = Field(..., min_length=1)

pool = ConnectionPool(
    conninfo=DSN,
    min_size=1,
    max_size=10,
    kwargs={"connect_timeout": 5},
    configure=_configure_conn,
)
embeddings = Embeddings.Embed()
@asynccontextmanager
async def lifespan(app: FastAPI):
    global embeddings
    print("Loading embedding model...")
    embeddings.load()
    print("Embedding model loaded.")
    yield
    embeddings = None

app = FastAPI(lifespan=lifespan)

@app.on_event

@app.get("/health")
def health():
    return {"ok": True}


@app.get("/health/db")
def health_db():
    try:
        with pool.connection() as conn:
            v = conn.execute("SELECT tablename\nFROM pg_catalog.pg_tables\nWHERE schemaname = 'public'\nORDER BY tablename;").fetchone()[0]
        return {"ok": True, "db": v}
    except psycopg.OperationalError as e:
        raise HTTPException(status_code=503, detail=f"db connection failed: {e}")

@app.get("/health/embed")
def health_embed():
    try:
        test_text = "test"
        vec = embeddings.embed(test_text)
        return {"ok": True, "embed_dim": len(vec)}
    except Exception as e:
        raise HTTPException(status_code=503, detail=f"embedding failed: {e}")
    
@app.post("/shoot")
def shoot(body: ShootRequest_Payload):
    text = body.text
    vec = embeddings.embed(text)
    apiresult = ""
    try:
        with pool.connection() as conn:
            conn.execute(f"INSERT INTO {TABLE_NAME} (text, embedding) VALUES (%s, %s::vector)", (text, vec))
            apiresult = f"text: {text}, embeddings vectors: {vec}"
        return {"ok": True, "result": apiresult}
    except Exception as e:
        return {"ok": False, "error": str(e)}
    
@app.post("/find")
def find(body: ShootRequest_Payload):
    text = body.text
    vec = embeddings.embed(text)
    results = []
    try:
        with pool.connection() as conn:
            rows = conn.execute(f"SELECT id, text, metadata, embedding <-> %s::vector AS distance FROM {TABLE_NAME} ORDER BY embedding <-> %s::vector LIMIT 5", (vec, vec)).fetchall()
            for row in rows:
                results.append({
                    "id": row[0],
                    "text": row[1],
                    "metadata": row[2],
                    "distance": row[3],
                })
        return {"ok": True, "results": results}
    except Exception as e:
        return {"ok": False, "error": str(e)}
