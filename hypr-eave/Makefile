# Spike: refraction behind eave's surfaces. Built against the installed Hyprland headers,
# so it only loads into the exact Hyprland it was built for.
PKGS = pixman-1 libdrm hyprland pangocairo libinput libudev wayland-server xkbcommon

hypr-eave.so: main.cpp
	$(CXX) -shared -fPIC -O2 -g -std=c++2b main.cpp -o $@ $$(pkg-config --cflags $(PKGS))

clean:
	rm -f hypr-eave.so

.PHONY: clean
