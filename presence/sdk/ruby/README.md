# arugula_presence (Ruby)

Sign arugula.stream room tokens on your server. openssl, json and base64
from the standard library; Ruby 3.0+.

Install (from this repo, until it's on RubyGems):

    cd presence/sdk/ruby && gem build arugula_presence.gemspec && gem install arugula_presence-0.1.0.gem
    # or in a Gemfile: gem "arugula_presence", path: "path/to/arugula/presence/sdk/ruby"

```ruby
ArugulaPresence.create_room_token(app: "acme", secret: ENV.fetch("ARUGULA_SECRET"), room: "design-team", user_id: user.id.to_s, name: user.name)
```

Keywords: `app:`, `secret:`, `room:`, `user_id:`, `name:` (required);
`look: { character:, colour: }`, `ttl:` (seconds, default 3600, at most
86400), `now:` (tests). Raises `ArgumentError` on bad input.

Rails controller:

```ruby
class PresenceTokensController < ApplicationController
  before_action :authenticate_user!

  def create
    room = params.require(:room)
    return head :forbidden unless current_user.team_rooms.include?(room) # your rule
    render json: { token: ArugulaPresence.create_room_token(app: "acme", secret: ENV.fetch("ARUGULA_SECRET"), room: room, user_id: current_user.id.to_s, name: current_user.name) }
  end
end
```

Tests: `ruby -Ilib test/test_token.rb` (every token in `../vectors.json`,
byte for byte). `mise.toml` pins a Ruby for this directory.
