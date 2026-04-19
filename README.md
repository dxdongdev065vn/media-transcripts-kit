<p align="center">
  <img src="assets/logo/logo.png" width="96" alt="My Media Kit logo" />
</p>

<h1 align="center">My Media Kit</h1>

<p align="center">
  Turn any video or podcast into readable, translatable, searchable text.<br/>
  Transcribe, translate, summarize, chapters, viral clips — locally (Apple Silicon) or via cloud APIs.
</p>

<p align="center">
  <a href="https://github.com/phuc-nt/my-media-kit/stargazers"><img src="https://img.shields.io/github/stars/phuc-nt/my-media-kit?style=social" alt="GitHub stars" /></a>
  <a href="https://github.com/phuc-nt/my-media-kit/releases/latest"><img src="https://img.shields.io/github/v/release/phuc-nt/my-media-kit" alt="Latest release" /></a>
  <img src="https://img.shields.io/badge/platform-macOS%20%7C%20Windows-blue" alt="platform" />
  <img src="https://img.shields.io/badge/license-MIT-green" alt="license" />
</p>

---

## Download

Grab the installer for your OS from the **[latest release](https://github.com/phuc-nt/my-media-kit/releases/latest)**:

- **macOS (Apple Silicon):** `.dmg`
- **Windows:** `.msi` or `.exe`

## Docs

- 📖 **[User Guide (Vietnamese)](https://phuc-nt.github.io/my-media-kit/user-guide/)** — install, features, screenshots
- 🏛️ **[Architecture Decisions](docs/architecture-decisions.md)**
- 🗺️ **[Roadmap / Backlog](docs/backlog.md)**

## Build from source

```bash
npm install
npm run dev
```

Requires Rust 1.80+, Node 20+, `ffmpeg`/`ffprobe`. On Apple Silicon for local MLX: `pip install mlx-lm mlx-whisper`.

## Star history

<a href="https://www.star-history.com/#phuc-nt/my-media-kit&Date">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/svg?repos=phuc-nt/my-media-kit&type=Date&theme=dark" />
    <source media="(prefers-color-scheme: light)" srcset="https://api.star-history.com/svg?repos=phuc-nt/my-media-kit&type=Date" />
    <img alt="Star history chart" src="https://api.star-history.com/svg?repos=phuc-nt/my-media-kit&type=Date" />
  </picture>
</a>

## License

[MIT](LICENSE) — built with [Tauri v2](https://tauri.app/).
