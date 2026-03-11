.PHONY: all clean

REPO_ROOT = ../..
LIB_DIR = $(REPO_ROOT)/target/release

all: demo

build-lib:
	cd $(REPO_ROOT) && cargo build --release -p risk-gate --features ffi

demo: build-lib main.c
	gcc -O2 -o demo main.c -L$(LIB_DIR) -lrisk_gate -lpthread -ldl -lm
	DYLD_LIBRARY_PATH=$(LIB_DIR) LD_LIBRARY_PATH=$(LIB_DIR) ./demo

clean:
	rm -f demo
