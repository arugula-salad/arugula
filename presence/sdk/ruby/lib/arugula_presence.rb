# frozen_string_literal: true

# Sign an arugula.stream room token on your server (PROTOCOL.md, "Room
# token"): an HS256 JWT with the claims in a fixed order and compact JSON,
# so every arugula SDK makes the same bytes for the same inputs
# (../vectors.json). openssl and json from the standard library (base64
# is a bundled gem since Ruby 3.4, so base64url is done with pack).

require "json"
require "openssl"

module ArugulaPresence
  VERSION = "0.1.0"

  ROOM = /\A[A-Za-z0-9_.:-]{1,128}\z/
  SUB = /\A[A-Za-z0-9_.:@-]{1,128}\z/
  # Control characters (and the two line separators some JSON encoders
  # escape) aren't names: refusing them keeps every SDK's JSON identical.
  CONTROL = /[\u0000-\u001f\u007f  ]/
  MAX_TTL = 86_400
  HEADER = '{"alg":"HS256","typ":"JWT"}'

  module_function

  # A room token for +user_id+ (called +name+) in +room+, signed with your
  # app's +secret+. +look+ is an optional { character:, colour: } (each
  # 0–255); +ttl+ seconds, at most 86400. Raises ArgumentError on bad input.
  def create_room_token(app:, secret:, room:, user_id:, name:, look: nil, ttl: 3600, now: nil)
    check(app.is_a?(String) && app.match?(ROOM), "app: your app's id, 1–128 of A–Z a–z 0–9 _ . : -")
    check(secret.is_a?(String) && !secret.empty?, "secret: your app's secret")
    check(room.is_a?(String) && room.match?(ROOM), "room: 1–128 of A–Z a–z 0–9 _ . : -")
    check(user_id.is_a?(String) && user_id.match?(SUB), "user_id: 1–128 of A–Z a–z 0–9 _ . : @ -")
    check(name.is_a?(String) && name.valid_encoding? && !name.empty? && name.length <= 80 && !name.match?(CONTROL),
          "name: 1–80 characters, no control characters")
    check(ttl.is_a?(Integer) && ttl.between?(1, MAX_TTL), "ttl: 1–86400 seconds")
    look &&= normalise(look)
    check(look != :error, "look: { character:, colour: }, each 0–255")

    at = now || Time.now.to_i
    claims = +%({"app":#{str(app)},"room":#{str(room)},"sub":#{str(user_id)},"name":#{str(name)},"exp":#{at + ttl})
    claims << %(,"look":{"character":#{look[:character]},"colour":#{look[:colour]}}) if look
    claims << "}"
    body = "#{b64(HEADER)}.#{b64(claims)}"
    "#{body}.#{b64(OpenSSL::HMAC.digest('SHA256', secret, body))}"
  end

  def normalise(look)
    return :error unless look.is_a?(Hash)

    ch = look[:character] || look["character"]
    co = look[:colour] || look["colour"]
    byte = ->(v) { v.is_a?(Integer) && v.between?(0, 255) }
    byte.(ch) && byte.(co) ? { character: ch, colour: co } : :error
  end

  def check(ok, why)
    raise ArgumentError, why unless ok
  end

  def str(s) = JSON.generate(s)

  def b64(data) = [data].pack("m0").tr("+/", "-_").delete("=")
end
