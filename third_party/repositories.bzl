"""
Loads third party repositories.
"""

load("//third_party/alsa:repositories.bzl", "alsa_repositories")
load("//third_party/binaries:repositories.bzl", "binary_repositories")
load("//third_party/libssh2:repositories.bzl", "libssh2_repositories")
load("//third_party/openssl:repositories.bzl", "openssl_repositories")
load("//third_party/pcre:repositories.bzl", "pcre_repositories")

def third_party_repositories():
    """Download dependencies for third-party libraries."""
    libssh2_repositories()
    pcre_repositories()
    openssl_repositories()
    binary_repositories()
    alsa_repositories()
