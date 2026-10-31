go := env("GO", "go")

default:
    @just --list

build:
    CGO_ENABLED=0 {{go}} build -o bin/synology-telegram-bot ./cmd/bot

vet:
    {{go}} vet ./...

test:
    {{go}} test ./... -cover

# Fails if any function (outside cmd/ and tools/) has a CRAP score >= 30.
crap:
    {{go}} test ./... -coverprofile=cover.out >/dev/null
    {{go}} run ./tools/crap -profile cover.out -max 30 -skip cmd/,tools/
