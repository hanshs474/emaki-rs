# emaki

Official SDK for emaki (emaki.ai), the Japanese-language AI video generator for Seedance 2.5 and Seedance 2.0. Generate video and images from your own code with one API key: [Seedance 2.5](https://emaki.ai/seedance-2-5), [Seedance 2.0](https://emaki.ai/seedance-2), [image to video](https://emaki.ai/image-to-video), plus Nano Banana 2, Seedream and GPT Image for stills. To sharpen a finished clip, use the [AI video upscaler](https://emaki.ai/video-upscaler) on the site.

日本語での使い方は [シーダンス](https://emaki.ai/) へ。

```rust
let clip = emaki::generate_video("a paper lantern drifting over a night river, slow dolly", &emaki::Params::new())?;
let img = emaki::generate_image("a ramen shop at dusk, isometric", &emaki::Params::new())?;
```

Each call submits the run, waits for it and returns the URL of the finished file.

## Install

```bash
cargo add emaki
```

Blocking client on `ureq`; no async runtime needed.

## API key

Sign in on [emaki.ai](https://emaki.ai/?utm_source=crates.io&utm_medium=package), create a key at [emaki.ai/settings/apikeys](https://emaki.ai/settings/apikeys), then export it:

```bash
export EMAKI_API_KEY=sk-...
```

A key runs on your account exactly like the site does: same models, same review, and every run costs credits on your emaki account, the same price as on the website.

## Video

The default video model is `seedance_2_5` (Seedance 2.5).

```rust
use emaki::{generate_video_with, Params};

let clip = generate_video_with(
    "a fox trotting through fresh snow at dawn, tracking shot",
    emaki::MODEL_VIDEO, // "seedance_2_5"
    &Params::new().set("duration", 8).set("resolution", "720p").set("aspect_ratio", "9:16"),
)?;

// image to video: the image becomes the first frame
let clip = generate_video_with("the camera slowly orbits the statue", "seedance_2",
    &Params::new().set("image", "https://example.com/statue.jpg"))?;
```

| model | parameters |
|---|---|
| `seedance_2_5` | `duration` 4-30 · `resolution` 480p / 720p / 1080p · `aspect_ratio` 16:9, 4:3, 1:1, 3:4, 9:16, 21:9, adaptive · `generate_audio` · `image` (first frame) · `last_frame_image` |
| `seedance_2` | `duration` 4-15 · `resolution` 480p / 720p / 1080p · `aspect_ratio` · `generate_audio` · `image` |
| `seedance_2_fast`, `seedance_2_mini` | `duration` 4-15 · `resolution` 480p / 720p · `aspect_ratio` · `generate_audio` · `image` |
| `minimax_h3` | `duration` 5-15 · `aspect_ratio` · `image` |
| `veo_3_1` | `resolution` 720p / 1080p / 4k · `aspectRatio` 16:9 / 9:16 · `image` |

Parameters you leave out take the same defaults as the site.

## Images

The default image model is `nano_banana_2_lite`, the cheapest one.

```rust
let url = emaki::generate_image_with("isometric diorama of a tea house", "nano_banana_2",
    &emaki::Params::new().set("aspect_ratio", "16:9").set("resolution", "2K"))?;
```

Image models: `nano_banana_2_lite`, `nano_banana_2` (`resolution` 1K / 2K / 4K), `seedream_5_lite` (`quality` basic / high / ultra), `gpt_image_2` (`resolution` 1K / 2K / 4K); all take `aspect_ratio`.

## Lower level

```rust
let client = emaki::Client::from_env()?;
let id = client.submit("a koi pond in the rain", "seedance_2_mini", &emaki::Params::new())?;
let st = client.status(&id)?;          // st.status: "generating" | "completed" | "failed"
let left = client.credits()?;          // remaining credits on the account
```

## Errors worth catching

- `Error::Auth`: no key, or the key is wrong or deleted
- `Error::Quota`: the account is out of credits
- `Error::Plan`: that model unlocks after a credit purchase
- `Error::Busy`: a generation on this account is already in progress
- `Error::Rejected`: the content filter refused the prompt or image; reword it rather than retrying
- `Error::Validation`: a parameter the API did not accept (the message says which)
- `Error::Failed`: the run ended as failed; carries the `failureCode`
- `Error::Timeout`: the deadline passed (15 minutes by default); the run may still finish in your library

Every finished run also lands in your library on emaki.ai.

MIT licensed. Made by [emaki](https://emaki.ai/?utm_source=crates.io&utm_medium=package), the Japanese-language home for Seedance.
