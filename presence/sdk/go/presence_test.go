package presence

import (
	"encoding/base64"
	"encoding/json"
	"os"
	"strings"
	"testing"
	"time"
)

type input struct {
	App      string `json:"app"`
	Secret   string `json:"secret"`
	Room     string `json:"room"`
	UserID   string `json:"user_id"`
	UserName string `json:"user_name"`
	Look     *struct {
		Character int `json:"character"`
		Colour    int `json:"colour"`
	} `json:"look"`
	TTL *int  `json:"ttl"`
	Now int64 `json:"now"`
}

type vector struct {
	Name  string `json:"name"`
	Input input  `json:"input"`
	Token string `json:"token"`
}

func opts(i input) TokenOptions {
	o := TokenOptions{App: i.App, Secret: i.Secret, Room: i.Room, UserID: i.UserID, Name: i.UserName, Now: time.Unix(i.Now, 0)}
	if i.Look != nil {
		o.Look = &Look{Character: i.Look.Character, Colour: i.Look.Colour}
	}
	if i.TTL != nil {
		o.TTL = time.Duration(*i.TTL) * time.Second
		if *i.TTL == 0 {
			// Zero means "an hour" here; the vector means "zero seconds".
			o.TTL = -1
		}
	}
	return o
}

func load(t *testing.T) (valid, invalid []vector) {
	b, err := os.ReadFile("../vectors.json")
	if err != nil {
		t.Fatal(err)
	}
	var v struct{ Valid, Invalid []vector }
	if err := json.Unmarshal(b, &v); err != nil {
		t.Fatal(err)
	}
	return v.Valid, v.Invalid
}

func TestVectors(t *testing.T) {
	valid, invalid := load(t)
	for _, v := range valid {
		got, err := CreateRoomToken(opts(v.Input))
		if err != nil || got != v.Token {
			t.Errorf("%s: got %q, %v; want %q", v.Name, got, err, v.Token)
		}
	}
	for _, v := range invalid {
		if _, err := CreateRoomToken(opts(v.Input)); err == nil {
			t.Errorf("%s: should refuse", v.Name)
		}
	}
}

func TestNowDefaultsToTheClock(t *testing.T) {
	tok, err := CreateRoomToken(TokenOptions{App: "acme", Secret: "s", Room: "r", UserID: "u", Name: "A"})
	if err != nil {
		t.Fatal(err)
	}
	p, _ := base64.RawURLEncoding.DecodeString(strings.Split(tok, ".")[1])
	var c struct{ Exp int64 }
	_ = json.Unmarshal(p, &c)
	if d := c.Exp - (time.Now().Unix() + 3600); d < -2 || d > 2 {
		t.Errorf("exp %d", c.Exp)
	}
}
