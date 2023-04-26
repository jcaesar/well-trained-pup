group "default" {
  targets = ["linuxi", "linuxa64"]
}

target "tag" {
  tags = ["docker.io/liftm/well-trained-pup"]
}

target "linuxi" {
  inherits = ["tag"]
  platforms = ["linux/amd64"]
  args = {
    CARGO_BUILD_TARGET = "x86_64-unknown-linux-musl"
  }
}

target "linuxa64" {
  inherits = ["tag"]
  platforms = ["linux/arm64"]
  args = {
    CARGO_BUILD_TARGET = "aarch64-unknown-linux-musl"
  }
}
