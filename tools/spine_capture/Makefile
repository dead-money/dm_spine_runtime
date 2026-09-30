# Links spine-cpp 4.3 into the golden-capture and benchmark CLIs.
#
# SPINE_RUNTIMES must be an upstream/4.3 checkout at the pinned commit in
# ../../docs/SPINE_4_3_UPGRADE.md, not the deadmoney/* branches.
#
#   make SPINE_RUNTIMES=../../../spine-runtimes-4.3

SPINE_RUNTIMES ?= ../../../spine-runtimes-4.3
SPINE_CPP := $(SPINE_RUNTIMES)/spine-cpp
OBJDIR    := build
BIN       := $(OBJDIR)/spine_capture
SYNTH_BIN := $(OBJDIR)/spine_synthetic

CXX      ?= g++
CXXFLAGS ?= -std=c++11 -O2 -Wall -Wextra -fno-exceptions -fno-rtti
CXXFLAGS += -isystem $(SPINE_CPP)/include
LDFLAGS  ?=

SPINE_SRCS := $(wildcard $(SPINE_CPP)/src/spine/*.cpp)
SPINE_OBJS := $(patsubst $(SPINE_CPP)/src/spine/%.cpp,$(OBJDIR)/spine/%.o,$(SPINE_SRCS))
OBJS       := $(OBJDIR)/main.o $(SPINE_OBJS)
SYNTH_OBJS := $(OBJDIR)/synthetic.o $(SPINE_OBJS)

all: $(BIN) $(SYNTH_BIN)

$(BIN): $(OBJS)
	$(CXX) $^ -o $@ $(LDFLAGS)

$(SYNTH_BIN): $(SYNTH_OBJS)
	$(CXX) $^ -o $@ $(LDFLAGS)

$(OBJDIR)/main.o: main.cpp | $(OBJDIR)
	$(CXX) $(CXXFLAGS) -c $< -o $@

$(OBJDIR)/synthetic.o: synthetic.cpp | $(OBJDIR)
	$(CXX) $(CXXFLAGS) -c $< -o $@

$(OBJDIR)/spine/%.o: $(SPINE_CPP)/src/spine/%.cpp | $(OBJDIR)/spine
	$(CXX) $(CXXFLAGS) -w -c $< -o $@

$(OBJDIR):
	mkdir -p $(OBJDIR)

$(OBJDIR)/spine: | $(OBJDIR)
	mkdir -p $(OBJDIR)/spine

clean:
	rm -rf $(OBJDIR)

.PHONY: all clean
