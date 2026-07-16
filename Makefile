.PHONY: build test bench bench-report corpus clean

build:
	cargo build --workspace

test:
	cargo test --workspace

bench:
	python3 bench/run_bench.py

bench-report:
	python3 bench/run_bench.py --dashboard-only

corpus:
	python3 scripts/gen_corpus.py

clean:
	cargo clean
