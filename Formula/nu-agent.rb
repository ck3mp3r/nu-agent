class NuAgent < Formula
  desc "Nushell agent plugin and CLI"
  homepage "https://github.com/ck3mp3r/nu-agent"
  version "0.1.0"
  license "GPL-2.0"

  on_macos do
    if Hardware::CPU.arm?
      url "https://github.com/ck3mp3r/nu-agent/releases/download/v0.1.0/nu-agent-0.1.0-aarch64-darwin.tgz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    else
      url "https://github.com/ck3mp3r/nu-agent/releases/download/v0.1.0/nu-agent-0.1.0-x86_64-darwin.tgz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    end
  end

  on_linux do
    if Hardware::CPU.intel?
      url "https://github.com/ck3mp3r/nu-agent/releases/download/v0.1.0/nu-agent-0.1.0-x86_64-linux.tgz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    elsif Hardware::CPU.arm?
      url "https://github.com/ck3mp3r/nu-agent/releases/download/v0.1.0/nu-agent-0.1.0-aarch64-linux.tgz"
      sha256 "0000000000000000000000000000000000000000000000000000000000000000"
    end
  end

  def install
    bin.install "nu_plugin_agent"
    bin.install "agent"
  end

  def caveats
    <<~EOS
      The `agent` CLI is installed to #{HOMEBREW_PREFIX}/bin and is on your PATH.

      To use the Nushell plugin, add Homebrew's bin directory to your plugin
      search path and register the plugin:

        $env.NU_PLUGIN_DIRS = ($env.NU_PLUGIN_DIRS | append "#{HOMEBREW_PREFIX}/bin")
        plugin add nu_plugin_agent
        plugin use agent
    EOS
  end

  test do
    system "#{bin}/agent", "--help"
    system "#{bin}/nu_plugin_agent", "--help"
  end
end
