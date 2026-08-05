interface EncodedVideoChunksHandle {
  readonly chunk_count: number;
  readonly description: Uint8Array | undefined;
  timestamp_us(index: number): number;
  is_key(index: number): boolean;
  data(index: number): Uint8Array;
}

interface DecodedVideoFramesHandle {
  readonly frame_count: number;
  timestamp_us(index: number): number;
  luma_plane(index: number): Uint8Array;
}

declare global {
  interface Window {
    mediawayE2e: {
      browserPkg: {
        error?: string;
        mux: { bytes: number; recovered: number; streams: string[] };
        video: { bytes?: number; packets?: number; codecs?: string[]; skipped?: string; error?: string };
        audio: { bytes?: number; packets?: number; codecs?: string[]; skipped?: string; error?: string };
        decodedVideo: { packets?: number; frames?: number; width?: number; height?: number; displayWidth?: number; displayHeight?: number; skipped?: string; error?: string };
        decodedAudio: { packets?: number; frames?: number; sampleRate?: number; numberOfChannels?: number; skipped?: string; error?: string };
      };
      iso: {
        wasm_mux_demux_smoke: () => number;
        wasm_mux_av_bytes: () => Uint8Array;
        wasm_mux_vp9_bytes: () => Uint8Array;
        wasm_mux_vp9_demux_smoke: () => string;
        default: () => Promise<void>;
      };
      enc: {
        is_webcodecs_av_supported: () => Promise<boolean>;
        webcodecs_av_fmp4_smoke: () => Promise<Uint8Array>;
        is_webcodecs_video_codec_supported: (codec: string) => Promise<boolean>;
        encode_video_frames: (
          codec: string,
          width: number,
          height: number,
          bitrateBps: number,
          lumas: Uint8Array,
          timestampsUs: Float64Array,
        ) => Promise<EncodedVideoChunksHandle>;
        fmp4_packet_count: (bytes: Uint8Array) => number;
        default: () => Promise<void>;
      };
      dec: {
        is_webcodecs_video_decode_supported: (
          codec: string,
          width: number,
          height: number,
        ) => Promise<boolean>;
        decode_video_chunks: (
          codec: string,
          width: number,
          height: number,
          description: Uint8Array | undefined,
          chunkData: Uint8Array,
          chunkOffsets: Uint32Array,
          chunkLengths: Uint32Array,
          chunkTimestampsUs: Float64Array,
          chunkIsKey: Uint8Array,
        ) => Promise<DecodedVideoFramesHandle>;
        default: () => Promise<void>;
      };
      dev: {
        device_selection_policy: () => string;
        UserMediaPreferences: new (video: boolean, audio: boolean) => object;
        DisplayCapturePreferences: new () => object;
        open_user_media: (prefs: object) => Promise<MediaStream>;
        open_display_capture: (prefs: object) => Promise<MediaStream>;
        media_stream_video_track_count: (stream: MediaStream) => number;
        default: () => Promise<void>;
      };
    };
  }
}

export {};
