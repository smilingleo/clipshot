APP_NAME = ClipShot
BUNDLE = $(APP_NAME).app
BINARY = clipshot
ICON_PNG = clipshot.png
ICON_ICNS = AppIcon.icns

CONTENTS = $(BUNDLE)/Contents
MACOS = $(CONTENTS)/MacOS
RESOURCES = $(CONTENTS)/Resources

.PHONY: build bundle icon clean install

build:
	cargo build --release

icon: $(ICON_PNG)
	mkdir -p AppIcon.iconset
	sips -z 16 16     $(ICON_PNG) --out AppIcon.iconset/icon_16x16.png
	sips -z 32 32     $(ICON_PNG) --out AppIcon.iconset/icon_16x16@2x.png
	sips -z 32 32     $(ICON_PNG) --out AppIcon.iconset/icon_32x32.png
	sips -z 64 64     $(ICON_PNG) --out AppIcon.iconset/icon_32x32@2x.png
	sips -z 128 128   $(ICON_PNG) --out AppIcon.iconset/icon_128x128.png
	sips -z 256 256   $(ICON_PNG) --out AppIcon.iconset/icon_128x128@2x.png
	sips -z 256 256   $(ICON_PNG) --out AppIcon.iconset/icon_256x256.png
	sips -z 512 512   $(ICON_PNG) --out AppIcon.iconset/icon_256x256@2x.png
	sips -z 512 512   $(ICON_PNG) --out AppIcon.iconset/icon_512x512.png
	sips -z 1024 1024 $(ICON_PNG) --out AppIcon.iconset/icon_512x512@2x.png
	iconutil -c icns AppIcon.iconset -o $(ICON_ICNS)
	rm -rf AppIcon.iconset

bundle: build icon
	rm -rf $(BUNDLE)
	mkdir -p $(MACOS) $(RESOURCES)
	cp target/release/$(BINARY) $(MACOS)/
	cp Info.plist $(CONTENTS)/
	cp $(ICON_ICNS) $(RESOURCES)/
	codesign --force --sign - --entitlements ClipShot.entitlements --deep $(BUNDLE)
	@echo "Built $(BUNDLE)"

install: bundle
	rm -rf /Applications/$(BUNDLE)
	cp -r $(BUNDLE) /Applications/
	@echo "Installed to /Applications/$(BUNDLE)"

clean:
	rm -rf $(BUNDLE) $(ICON_ICNS) AppIcon.iconset
	cargo clean
