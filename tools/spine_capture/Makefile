# spine_capture — builds a C++ tool that links spine-cpp and dumps
# setup-pose bone transforms to JSON. Used to generate Phase 2 golden
# fixtures for the Rust port.
#
# Source tree assumptions:
#   this Makefile lives at <runtime>/tools/spine_capture/
#   spine-cpp reference lives at <runtime>/../spine-runtimes/spine-cpp/
#
# Build:   make            (produces build/spine_capture)
# Clean:   make clean

SPINE_CPP := ../../../spine-runtimes/spine-cpp/spine-cpp
OBJDIR    := build
BIN       := $(OBJDIR)/spine_capture

CXX      ?= g++
CXXFLAGS ?= -std=c++11 -O2 -Wall -Wextra
CXXFLAGS += -I$(SPINE_CPP)/include
LDFLAGS  ?=

SPINE_SRCS := $(wildcard $(SPINE_CPP)/src/spine/*.cpp)
SPINE_OBJS := $(patsubst $(SPINE_CPP)/src/spine/%.cpp,$(OBJDIR)/spine/%.o,$(SPINE_SRCS))
OBJS       := $(OBJDIR)/main.o $(SPINE_OBJS)

all: $(BIN)

$(BIN): $(OBJS)
	$(CXX) $^ -o $@ $(LDFLAGS)

$(OBJDIR)/main.o: main.cpp | $(OBJDIR)
	$(CXX) $(CXXFLAGS) -c $< -o $@

$(OBJDIR)/spine/%.o: $(SPINE_CPP)/src/spine/%.cpp | $(OBJDIR)/spine
	$(CXX) $(CXXFLAGS) -c $< -o $@

$(OBJDIR):
	mkdir -p $(OBJDIR)

$(OBJDIR)/spine: | $(OBJDIR)
	mkdir -p $(OBJDIR)/spine

clean:
	rm -rf $(OBJDIR)

.PHONY: all clean
