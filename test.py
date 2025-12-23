from argon2 import PasswordHasher

ph = PasswordHasher(
	time_cost=3,        # 반복 횟수(느릴수록 강함
	memory_cost=64_000, # KiB 단위 (64_000 KiB ≈ 64MB)
	parallelism=1,
	hash_len=64,
	salt_len=16
)

print(ph.hash("host"))