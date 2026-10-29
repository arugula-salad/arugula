defmodule ArugulaPresence.MixProject do
  use Mix.Project

  def project do
    [
      app: :arugula_presence,
      version: "0.1.0",
      elixir: "~> 1.14",
      description: "Sign arugula.stream room tokens on your server (HS256, :crypto).",
      package: [licenses: ["MIT", "Apache-2.0"], links: %{}],
      deps: [{:jason, "~> 1.4"}]
    ]
  end

  def application, do: [extra_applications: [:crypto]]
end
