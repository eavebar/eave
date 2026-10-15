# hypr-eave: glass behind eave's islands, a Hyprland plugin. Built against the installed Hyprland
# headers, so it only loads into the exact Hyprland it was built for: rebuild after updating
# Hyprland (eave-build does).
PKGS     = pixman-1 libdrm hyprland pangocairo libinput libudev wayland-server xkbcommon
PROTOCOL = ../protocol/eave-glass-v1.xml
BUILD    = build
SOURCES  = $(wildcard src/*.cpp)
SHADERS  = $(wildcard shaders/*)
OBJECTS  = $(SOURCES:src/%.cpp=$(BUILD)/%.o) $(BUILD)/eave-glass-v1.o
# -MMD: each object also depends on the Hyprland headers it read, so a Hyprland update rebuilds.
CXXFLAGS += -std=c++2b -fPIC -O2 -g -MMD -MP -Wall -Wextra -Werror -Wno-unused-parameter -I$(BUILD) $(shell pkg-config --cflags $(PKGS))

hypr-eave.so: $(OBJECTS)
	$(CXX) -shared $^ -o $@

$(BUILD)/eave-glass-v1.cpp $(BUILD)/eave-glass-v1.hpp: $(PROTOCOL)
	@mkdir -p $(BUILD)
	hyprwayland-scanner $(PROTOCOL) $(BUILD)/

# Each shader as a raw string: SHADER_glass_frag for shaders/glass.frag.
$(BUILD)/shaders.inc: $(SHADERS) Makefile
	@mkdir -p $(BUILD)
	@for f in $(SHADERS); do \
		name=$$(basename $$f | tr . _); \
		printf '[[maybe_unused]] static const char* SHADER_%s = R"#(' $$name; cat $$f; printf ')#";\n'; \
	done > $@

$(BUILD)/eave-glass-v1.o: $(BUILD)/eave-glass-v1.cpp
	$(CXX) $(CXXFLAGS) -Wno-error -c $< -o $@

$(BUILD)/%.o: src/%.cpp src/*.hpp $(BUILD)/eave-glass-v1.hpp $(BUILD)/shaders.inc
	$(CXX) $(CXXFLAGS) -c $< -o $@

-include $(OBJECTS:.o=.d)

clean:
	rm -rf $(BUILD) hypr-eave.so

.PHONY: clean
