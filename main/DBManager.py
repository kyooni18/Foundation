import psycopg
from psycopg_pool import ConnectionPool
from fastapi import HTTPException
import random
import string
import hashlib
from argon2 import PasswordHasher
from argon2.exceptions import VerifyMismatchError

class DBManager:
	def __init__(self, 
			pool: ConnectionPool):
		self.pool: ConnectionPool = pool
		
		
	def health_check(self):
		try:
			with self.pool.connection() as conn:
				v = conn.execute("SELECT tablename\nFROM pg_catalog.pg_tables\nWHERE schemaname = 'public'\nORDER BY tablename;").fetchone()[0]
			return {"ok": True, "db": v}
		except psycopg.OperationalError as e:
			raise HTTPException(status_code=503, detail=f"db connection failed: {e}")
		
class KeyManager:
	def __init__(self, 
			pool: ConnectionPool):
		self.pool: ConnectionPool = pool
		self.ph = PasswordHasher(
 	    time_cost=3,        # 반복 횟수(느릴수록 강함)
 	    memory_cost=64_000, # KiB 단위 (64_000 KiB ≈ 64MB)
 	    parallelism=1,
 	    hash_len=64,
 	    salt_len=16,
		)

	def verify(self, api_key: str) -> bool:
		with self.pool.connection() as conn:
			row = conn.execute("SELECT hashed_key FROM auth_keys").fetchall()
			for r in row:
				try:
					self.ph.verify(r[0], api_key)
					print("verified")
					return True
				except VerifyMismatchError:
					continue
			return False
		
	def create(self, master_key: str) -> str:
		if not self.verify(master_key):
			return {"ok": False, "error": "Invalid master key"}
		random_key = "foundation_"
		for _ in range(64):
			random_key += random.choice(string.ascii_lowercase + string.ascii_uppercase + string.digits)
		mask = "foundation_" + random_key[11:15] + "*" * 60
		hashed_key = self.ph.hash(random_key)
		with self.pool.connection() as conn:
			conn.execute("INSERT INTO auth_keys (hashed_key, mask) VALUES (%s, %s)", (hashed_key, mask))
		return {"ok": True, "mask": mask, "api_key": random_key}
	
	def delete(self, api_key: str):
		
		
		with self.pool.connection() as conn:
			rows = conn.execute("SELECT hashed_key FROM auth_keys").fetchall()
			for r in rows:
				try:
					self.ph.verify(r[0], api_key)
					hashed_key = r[0]
					break
				except VerifyMismatchError:
					continue
			conn.execute("DELETE FROM auth_keys WHERE hashed_key = %s", (hashed_key,))
		
	
	def list_keys(self):
		with self.pool.connection() as conn:
			rows = conn.execute("SELECT mask, created_at FROM auth_keys ORDER BY created_at DESC").fetchall()
			result = ""
			for row in rows:
				result += f"mask: {row[0]}, created_at: {row[1].isoformat()}\n"
		return result