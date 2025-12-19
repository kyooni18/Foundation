FROM python:3.12-slim

WORKDIR /main
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt
COPY Embeddings.py /main/Embeddings.py
COPY asd.py /main/asd.py
COPY main.py /main/main.py
