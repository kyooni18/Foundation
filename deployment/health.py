"""Authenticated health probe; never places credentials in argv or output."""
import json
import urllib.request
with open('/credentials/user.token') as f:
    token = f.read().strip()
request = urllib.request.Request('http://127.0.0.1:7823/v1/queue_status', data=b'{}',
    headers={'Authorization': 'Bearer ' + token, 'Content-Type': 'application/json'})
with urllib.request.urlopen(request, timeout=8) as response:
    result = json.load(response)
    if not result.get('ok'):
        raise SystemExit(1)
