# Pre-built binaries are injected into binaries/${TARGETARCH}/lemma by CI.
# See .github/workflows/release.yml publish-docker job.
FROM scratch
ARG TARGETARCH
COPY binaries/${TARGETARCH}/lemma /usr/local/bin/lemma
ENTRYPOINT ["/usr/local/bin/lemma"]
CMD ["--help"]
EXPOSE 8012
