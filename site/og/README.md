# Link-preview image

`og.html` is the 1200×630 image shown when remap75.app or web.remap75.app is shared
(`../public/og.png`). To regenerate it after changing the page or the screenshots:

```sh
cd site/og
"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" --headless=new --disable-gpu \
  --hide-scrollbars --window-size=1200,630 --screenshot=../public/og.png "file://$PWD/og.html"
```

Then `bun run deploy`, and refresh the cached preview on Facebook / LinkedIn if needed.
