# arugula.stream/presence (Go)

Sign arugula.stream room tokens on your server. Standard library only.

Install (from this repo, until the module path is served): in your
`go.mod`,

    require arugula.stream/presence v0.1.0
    replace arugula.stream/presence => ./path/to/arugula/presence/sdk/go

```go
token, err := presence.CreateRoomToken(presence.TokenOptions{
	App: "acme", Secret: os.Getenv("ARUGULA_SECRET"), Room: "design-team",
	UserID: user.ID, Name: user.Name,
})
```

Fields: `App`, `Secret`, `Room`, `UserID`, `Name` (required); `Look
*Look`, `TTL time.Duration` (0 means an hour; at most 24 h), `Now` (tests).
Returns an error on bad input.

net/http endpoint:

```go
http.HandleFunc("POST /api/presence-token", func(w http.ResponseWriter, r *http.Request) {
	user := currentUser(r) // your session
	var ask struct{ Room string }
	if json.NewDecoder(r.Body).Decode(&ask) != nil || !user.InTeam(ask.Room) {
		http.Error(w, "no", http.StatusForbidden)
		return
	}
	token, err := presence.CreateRoomToken(presence.TokenOptions{App: "acme", Secret: os.Getenv("ARUGULA_SECRET"), Room: ask.Room, UserID: user.ID, Name: user.Name})
	if err != nil {
		http.Error(w, err.Error(), http.StatusBadRequest)
		return
	}
	json.NewEncoder(w).Encode(map[string]string{"token": token})
})
```

Tests: `go test ./...` (every token in `../vectors.json`, byte for byte).
