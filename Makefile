GO ?= go

.PHONY: help build run install check clean

help:
	@printf '%s\n' \
		'make build             Build bin/donn' \
		'make run [ARGS="..."]  Run the TUI or a donn subcommand' \
		'make install           Install donn into GOBIN or GOPATH/bin' \
		'make check             Check formatting, vet, and build' \
		'make clean             Remove bin/ build artifacts'

build:
	CGO_ENABLED=0 $(GO) build -trimpath -ldflags="-s -w" -o bin/donn ./cmd/donn

run:
	$(GO) run ./cmd/donn $(ARGS)

install:
	CGO_ENABLED=0 $(GO) install -trimpath -ldflags="-s -w" ./cmd/donn

check:
	@test -z "$$(gofmt -l cmd internal)" || { gofmt -l cmd internal; exit 1; }
	$(GO) vet ./...
	$(MAKE) build

clean:
	rm -rf bin/
