# frozen_string_literal: true

require_relative "lib/arugula_presence"

Gem::Specification.new do |s|
  s.name = "arugula_presence"
  s.version = ArugulaPresence::VERSION
  s.summary = "Sign arugula.stream room tokens on your server (HS256, standard library only)."
  s.authors = ["Arugula"]
  s.licenses = ["MIT", "Apache-2.0"]
  s.files = ["lib/arugula_presence.rb"]
  s.required_ruby_version = ">= 3.0"
end
