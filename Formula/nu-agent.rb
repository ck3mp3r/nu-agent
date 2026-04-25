class NuAgent < Formula
  desc "Nushell plugin for LLM agent interactions"
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
  end

  test do
    system "#{bin}/nu_plugin_agent", "--help"
  end
end
