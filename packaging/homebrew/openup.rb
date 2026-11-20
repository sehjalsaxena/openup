class Openup < Formula
  desc "Terminal dashboard and background keeper for campus captive portals"
  homepage "https://github.com/sehjalsaxena/openup"
  version "0.1.0"

  on_macos do
    on_arm do
      url "https://github.com/sehjalsaxena/openup/releases/download/v0.1.0/openup-darwin-arm64.tar.gz"
    end
    on_intel do
      url "https://github.com/sehjalsaxena/openup/releases/download/v0.1.0/openup-darwin-amd64.tar.gz"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/sehjalsaxena/openup/releases/download/v0.1.0/openup-linux-arm64.tar.gz"
    end
    on_intel do
      url "https://github.com/sehjalsaxena/openup/releases/download/v0.1.0/openup-linux-amd64.tar.gz"
    end
  end

  def install
    bin.install "openup"
  end

  test do
    assert_match "OpenUp", shell_output("#{bin}/openup --version")
  end
end
