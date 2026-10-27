# AERIC orchestrator -- portable C99 build (gcc/clang).
# On Windows with MSVC, use build.bat instead.
CC      ?= cc
CFLAGS  ?= -std=c99 -O2 -Wall -Wextra -Wpedantic
LDLIBS  ?= -lm

MODEL   ?= server_capacity
SRC      = aeric.c aven_shim.c model_$(MODEL).c main.c
BIN      = aeric_demo

$(BIN): $(SRC) aeric.h model_$(MODEL).h
	$(CC) $(CFLAGS) $(SRC) $(LDLIBS) -o $(BIN)

# Regenerate the model translation unit from the .aven source. The emitter
# appends a demo main(); we strip it so the result links as a library unit.
# NOTE: emit WITHOUT --checked-c -- that mode calls exit() on a domain fault and
# would bypass AERIC's HOLD fallback.
regen-model:
	../target/release/avenc emit-c ../examples/$(MODEL).aven --traceable \
	  | sed '/^int main(void) {/,$$d' > model_$(MODEL).c

run: $(BIN)
	./$(BIN)

clean:
	rm -f $(BIN) *.o *.obj *.exe

.PHONY: regen-model run clean
