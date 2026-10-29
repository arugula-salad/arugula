# arugula-presence (Python)

Sign arugula.stream room tokens on your server. Standard library only,
Python 3.9+.

Install (from this repo, until it's on PyPI):

    pip install ./presence/sdk/python

    from arugula_presence import create_room_token
    token = create_room_token(app="acme", secret=os.environ["ARUGULA_SECRET"], room="design-team", user_id=user.id, name=user.name)

Keyword arguments: `app`, `secret`, `room`, `user_id`, `name` (required);
`look={"character": n, "colour": n}`, `ttl` (seconds, default 3600, at most
86400), `now` (tests). Raises `ValueError` on bad input.

FastAPI endpoint:

```python
import os
from fastapi import Depends, FastAPI, HTTPException
from pydantic import BaseModel
from arugula_presence import create_room_token

app = FastAPI()

class Ask(BaseModel):
    room: str

@app.post("/api/presence-token")
def presence_token(ask: Ask, user=Depends(current_user)):
    if ask.room not in user.teams:  # your rule for who may be where
        raise HTTPException(403)
    return {"token": create_room_token(app="acme", secret=os.environ["ARUGULA_SECRET"], room=ask.room, user_id=str(user.id), name=user.name)}
```

Tests: `python3 -m unittest discover -s tests` (every token in `../vectors.json`, byte for byte).
