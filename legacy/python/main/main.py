import os
from fastapi import FastAPI, HTTPException, Header
from contextlib import asynccontextmanager
from pydantic import BaseModel, Field
import psycopg
from psycopg_pool import ConnectionPool
from pgvector.psycopg import register_vector
import Embeddings
import DBManager

# ----------------------------- ENVS -------------------------------

TABLE_NAME = os.getenv("EMBEDDINGS_TABLE", "atoms_db")
EMBEDDING_DIM = os.getenv("EMBEDDING_DIM", "768")
POSTGRES_PASSWORD = os.getenv("POSTGRES_PASSWORD", "host")
POSTGRES_USER = os.getenv("POSTGRES_USER", "foundation")
POSTGRES_DB = os.getenv("POSTGRES_DB", "foundation_db1")
POSTGRES_PORT = os.getenv("POSTGRES_PORT", "5432")

# ----------------------------- Database Connection -------------------------------

DSN = os.getenv("DATABASE_URL", f"postgresql://{POSTGRES_USER}:{POSTGRES_PASSWORD}@localhost:{POSTGRES_PORT}/{POSTGRES_DB}")

def _configure_conn(conn: psycopg.Connection) -> None:
    register_vector(conn)

pool = ConnectionPool(
    conninfo=DSN,
    min_size=1,
    max_size=10,
    kwargs={"connect_timeout": 5},
    configure=_configure_conn,
)
# ----------------------------- Managers Initialization -------------------------------

db_manager = DBManager.DBManager(pool=pool)
key_manager = DBManager.KeyManager(pool=pool)
embed_manager = Embeddings.EmbedManager()

# ----------------------------- FastAPI Lifespan -------------------------------

@asynccontextmanager
async def lifespan(app: FastAPI):
    global embed_manager
    embed_manager.load()

    
    yield
    embed_manager = None

app = FastAPI(lifespan=lifespan)

# ----------------------------- POST Payload Models -------------------------------

class ShootRequest_Payload(BaseModel):
    text: str = Field(..., min_length=1)

class KeyCreation_Payload(BaseModel):
    api_key: str = Field(..., min_length=1)

# ----------------------------- API Keys Management Endpoints -------------------------------

@app.get("/keys/list")
def list_keys():
    if key_manager.list_keys() == "":
        with pool.connection() as conn:
            conn.execute("INSERT INTO auth_keys (hashed_key, mask) VALUES('$argon2id$v=19$m=64000,t=3,p=1$VHSUQ7l58UdHbrBltQfFKA$fMiRFZcgw8ri8W6bczyW8kODs6F+bOKROWqAcTDgqjyv8W7aPNKZyCebvkBtYVBomwb63B2bAXLQwE2Wz+AfSw', 'master_key')")
    return {"ok": True, "result": key_manager.list_keys()}
@app.post("/keys/create")
def create_key(body: KeyCreation_Payload):
    return key_manager.create(body.api_key)
@app.post("/keys/delete")
def delete_key(body: KeyCreation_Payload):
    key_manager.delete(body.api_key)
    return {"ok": True, "result": "Key deleted"}
@app.post("/keys/verify")
def verify_key(body: KeyCreation_Payload):
    is_valid = key_manager.verify(body.api_key)
    return {"ok": True, "valid": is_valid}

# ----------------------------- Health Check Endpoints -------------------------------

@app.get("/health")
def health():
    return {"ok": True}
@app.get("/health/db")
def health_db():
   return db_manager.health_check()
@app.get("/health/embed")
def health_embed():
    try:
        vec = embed_manager.embed("test")
        return {"ok": True, "embed_dim": len(vec)}
    except Exception as e:
        raise HTTPException(status_code=503, detail=f"embedding failed: {e}")
    
# ----------------------------- Foundation Endpoints -------------------------------

@app.post("/embed/text")
def embed(body: ShootRequest_Payload, Authorization: str = Header(None)):
    if key_manager.verify(Authorization.replace("Bearer ", "")) is False:
        raise HTTPException(status_code=401, detail="Invalid API Key")
    text = body.text
    try:
        vec = embed_manager.embed(text)
        return {"ok": True, "embedding": vec.tolist()}
    except Exception as e:
        raise HTTPException(status_code=503, detail=f"embedding failed: {e}")

@app.post("/add")
def add(body: ShootRequest_Payload, Authorization: str = Header(None)):
    if key_manager.verify(Authorization.replace("Bearer ", "")) is False:
        raise HTTPException(status_code=401, detail="Invalid API Key")
    text = body.text
    vec = embed_manager.embed(text)
    apiresult = ""
    try:
        with pool.connection() as conn:
            if conn.execute(f"SELECT id FROM {TABLE_NAME} WHERE text = %s", (text,)).fetchone() is not None:
                return {"ok": False, "error": "Text already exists"}
            conn.execute(f"INSERT INTO {TABLE_NAME} (text, embedding) VALUES (%s, %s::vector)", (text, vec))
            apiresult = f"text: {text}, embed_manager vectors: {vec}"
        return {"ok": True, "result": apiresult}
    except Exception as e:
        return {"ok": False, "error": str(e)}

@app.post("/delete")
def delete(body: ShootRequest_Payload , Authorization: str = Header(None)):
    if key_manager.verify(Authorization.replace("Bearer ", "")) is False:
        raise HTTPException(status_code=401, detail="Invalid API Key")
    text = body.text
    try:
        with pool.connection() as conn:
            conn.execute(f"DELETE FROM {TABLE_NAME} WHERE text = %s", (text,))
        return {"ok": True, "result": f"Deleted text: {text}"}
    except Exception as e:
        return {"ok": False, "error": str(e)}

@app.post("/find")
def find(body: ShootRequest_Payload , Authorization: str = Header(None)):
    if key_manager.verify(Authorization.replace("Bearer ", "")) is False:
        raise HTTPException(status_code=401, detail="Invalid API Key")
    text = body.text
    vec = embed_manager.embed(text)
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

# ----------------------------- END -------------------------------