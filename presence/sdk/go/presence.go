// Package presence signs arugula.stream room tokens on your server
// (PROTOCOL.md, "Room token"): an HS256 JWT with the claims in a fixed
// order and compact JSON, so every arugula SDK makes the same bytes for
// the same inputs (../vectors.json). Standard library only.
package presence

import (
	"bytes"
	"crypto/hmac"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"regexp"
	"strings"
	"time"
	"unicode/utf8"
)

// Look is a starting character and colour (each 0–255).
type Look struct {
	Character int
	Colour    int
}

// TokenOptions says whose token it is and for which room.
type TokenOptions struct {
	// App is your app's id on the relay; Secret its secret (server only).
	App    string
	Secret string
	// Room is 1–128 of A–Z a–z 0–9 _ . : -
	Room string
	// UserID is your id for them (1–128 of A–Z a–z 0–9 _ . : @ -), Name
	// what to call them (1–80 characters).
	UserID string
	Name   string
	// Look is optional.
	Look *Look
	// TTL is how long it lives, up to 24 h (0: an hour).
	TTL time.Duration
	// Now is when to count from (tests); zero: the clock.
	Now time.Time
}

var (
	roomRe = regexp.MustCompile(`\A[A-Za-z0-9_.:-]{1,128}\z`)
	subRe  = regexp.MustCompile(`\A[A-Za-z0-9_.:@-]{1,128}\z`)
)

const header = `{"alg":"HS256","typ":"JWT"}`

// control: control characters and the two line separators Go's encoder
// always escapes aren't names, so every SDK's JSON stays identical.
func control(s string) bool {
	return strings.IndexFunc(s, func(r rune) bool {
		return r < 0x20 || r == 0x7f || r == 0x2028 || r == 0x2029
	}) >= 0
}

func str(s string) string {
	var b bytes.Buffer
	e := json.NewEncoder(&b)
	e.SetEscapeHTML(false)
	_ = e.Encode(s)
	return strings.TrimSuffix(b.String(), "\n")
}

func b64(b []byte) string { return base64.RawURLEncoding.EncodeToString(b) }

// CreateRoomToken returns a room token (HS256 JWT) for the user in the
// room, or why not.
func CreateRoomToken(o TokenOptions) (string, error) {
	if !roomRe.MatchString(o.App) {
		return "", errors.New("App: your app's id, 1–128 of A–Z a–z 0–9 _ . : -")
	}
	if o.Secret == "" {
		return "", errors.New("Secret: your app's secret")
	}
	if !roomRe.MatchString(o.Room) {
		return "", errors.New("Room: 1–128 of A–Z a–z 0–9 _ . : -")
	}
	if !subRe.MatchString(o.UserID) {
		return "", errors.New("UserID: 1–128 of A–Z a–z 0–9 _ . : @ -")
	}
	if o.Name == "" || !utf8.ValidString(o.Name) || utf8.RuneCountInString(o.Name) > 80 || control(o.Name) {
		return "", errors.New("Name: 1–80 characters, no control characters")
	}
	ttl := o.TTL
	if ttl == 0 {
		ttl = time.Hour
	}
	if ttl%time.Second != 0 || ttl < time.Second || ttl > 24*time.Hour {
		return "", errors.New("TTL: whole seconds, 1 s to 24 h")
	}
	byteOK := func(v int) bool { return v >= 0 && v <= 255 }
	if o.Look != nil && !(byteOK(o.Look.Character) && byteOK(o.Look.Colour)) {
		return "", errors.New("Look: Character and Colour, each 0–255")
	}
	now := o.Now
	if now.IsZero() {
		now = time.Now()
	}
	claims := fmt.Sprintf(`{"app":%s,"room":%s,"sub":%s,"name":%s,"exp":%d`, str(o.App), str(o.Room), str(o.UserID), str(o.Name), now.Unix()+int64(ttl/time.Second))
	if o.Look != nil {
		claims += fmt.Sprintf(`,"look":{"character":%d,"colour":%d}`, o.Look.Character, o.Look.Colour)
	}
	claims += "}"
	body := b64([]byte(header)) + "." + b64([]byte(claims))
	m := hmac.New(sha256.New, []byte(o.Secret))
	m.Write([]byte(body))
	return body + "." + b64(m.Sum(nil)), nil
}
