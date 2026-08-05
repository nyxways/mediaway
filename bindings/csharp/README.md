# Mediaway for .NET

Native media capture, encoding, and container mux/demux for .NET, backed by
Mediaway's C ABI (`mediaway_ffi.dll`).

- Package and usage documentation: [`src/README.md`](src/README.md)
- NuGet packaging (ADR-0017): `src/Directory.Build.targets` bundles the staged
  DLLs under `runtime/win-x64/native` into the packages' `runtimes/win-x64/native`.

## Testing

The test projects under `tests/` exercise the **real** native library
(`mediaway_ffi.dll`) through the P/Invoke layer — no mocks. They are xunit-based
and assert-driven: `dotnet test` exits nonzero on any failure.

### Native library resolution

No NuGet native-asset packaging exists yet (ADR-0017 build sequence step 5), so
each test project copies the DLL into its own output directory at build time:

1. Prefer the release-staged copy at `runtime/win-x64/native/mediaway_ffi.dll`
   (populated by `tools/scripts/copy-native-dlls.ts` — this is what the release
   workflow's `native-assets` job stages and the RC-stage binding check
   downloads as the `native-dlls` artifact).
2. Fall back to the workspace's dev-time cargo output at
   `target/debug/mediaway_ffi.dll` so a local `dotnet test` works without a
   staging step.

Exactly one of the two conditions holds, so exactly one DLL is copied.

### RC-stage binding check

The release workflow's RC gate (added as a `bindings-tests` job) validates the
release-built DLL before publishing. From the repository root, with the
`native-dlls` artifact already staged:

```bash
dotnet test bindings/csharp/tests/Mediaway.Container.Tests/Mediaway.Container.Tests.csproj
```

or, from `bindings/csharp/`:

```bash
dotnet test tests/Mediaway.Container.Tests/Mediaway.Container.Tests.csproj
```

`Mediaway.Container.Tests` (`MuxRoundtripTests`) is the RC suite: it muxes 90
synthetic H.264 video packets and 90 synthetic AAC audio packets into a
fragmented MP4, demuxes the bytes back, and asserts 1:1 packet counts and
stream metadata. Pure CPU, no hardware, deterministic.

What must NOT run at RC:

- **`tests/Mediaway.Pipeline.Tests`** (`EncodeToMp4Tests`) — P/Invokes the same
  shipped `mediaway_ffi.dll` (there is no separate pipeline DLL), but its encode
  tests require a hardware-verified WMF/DX11 H.264 encoder backend and treat
  `EncoderUnavailableException` as a real failure rather than a skip (see the
  test's own doc comment). Encoder availability is not guaranteed on CI
  runners, so this suite is local-build/dev-machine verified only and stays out
  of the RC gate.
- **`tests/Mediaway.Device.Tests`** (`CaptureTests`) — requires real camera,
  microphone, and display (Screen/D3D11) hardware (see the test's own doc
  comment). Hardware is never available in CI; excluded from the RC gate by design.

Run the full local (non-hardware) check on a dev machine with an encoder
backend with:

```bash
dotnet test tests/Mediaway.Container.Tests/Mediaway.Container.Tests.csproj
dotnet test tests/Mediaway.Pipeline.Tests/Mediaway.Pipeline.Tests.csproj
```

## License

MIT OR Apache-2.0. Source: [github.com/nyxways/mediaway](https://github.com/nyxways/mediaway).
