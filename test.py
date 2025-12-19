import requests

url = "http://localhost:8000/shoot"
payload = {
	"text": "This is a test sentence for insertion."
}
headers = {
	"Content-Type": "application/json"
}

response = requests.post(url, json=payload, headers=headers)
print(response)