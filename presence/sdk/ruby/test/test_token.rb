# frozen_string_literal: true

# Every vector in ../../vectors.json, byte for byte, and every invalid
# input refused.
require "minitest/autorun"
require "json"
require_relative "../lib/arugula_presence"

class TokenTest < Minitest::Test
  VECTORS = JSON.parse(File.read(File.expand_path("../../vectors.json", __dir__)))

  def kwargs(i)
    k = { app: i["app"], secret: i["secret"], room: i["room"], user_id: i["user_id"], name: i["user_name"], now: i["now"] }
    k[:look] = i["look"] if i["look"]
    k[:ttl] = i["ttl"] if i["ttl"]
    k
  end

  def test_vectors
    VECTORS["valid"].each do |v|
      assert_equal v["token"], ArugulaPresence.create_room_token(**kwargs(v["input"])), v["name"]
    end
  end

  def test_invalid
    VECTORS["invalid"].each do |v|
      assert_raises(ArgumentError, v["name"]) { ArugulaPresence.create_room_token(**kwargs(v["input"])) }
    end
  end

  def test_now_defaults_to_the_clock
    t = ArugulaPresence.create_room_token(app: "acme", secret: "s", room: "r", user_id: "u", name: "A")
    part = t.split(".")[1].tr("-_", "+/")
    exp = JSON.parse((part + "=" * (-part.size % 4)).unpack1("m0"))["exp"]
    assert_in_delta Time.now.to_i + 3600, exp, 2
  end
end
