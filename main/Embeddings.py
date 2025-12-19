class Embed:
	def __init__(self):
		self.model = None
		self.dim = 1024
	
	def load(self):
		from sentence_transformers import SentenceTransformer
		import numpy as np
		if self.model == None:
			self.model = SentenceTransformer("ibm-granite/granite-embedding-278m-multilingual")
	
	def embed(self, text):
		if self.model == None:
			self.load()
		
		vec = self.model.encode(
			[text],
			batch_size=32,
			normalize_embeddings=True,
			truncate_dim=self.dim)[0]
		
		return vec