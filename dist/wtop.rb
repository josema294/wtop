class Wtop < Formula
  desc "Web-based real-time system monitor built with Rust"
  homepage "https://github.com/josema294/wtop"
  url "https://github.com/josema294/wtop/archive/refs/tags/v0.2.1.tar.gz"
  sha256 "FILL_IN_AFTER_RELEASE"
  license "AGPL-3.0-or-later"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args
    man1.install "dist/wtop.1"
  end

  def caveats
    <<~EOS
      To start wtop:
        wtop --localhost-only

      Then open http://127.0.0.1:3000 in your browser.

      Note: GPU monitoring via NVML requires NVIDIA drivers.
      Some Linux-specific metrics (/proc, /sys) are not available on macOS.
    EOS
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/wtop --version")
  end
end
