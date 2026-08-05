/**
 * Minimal ambient types for the WebCodecs surface @mediaway/browser uses.
 *
 * TypeScript's lib.dom still lacks WebCodecs (microsoft/TypeScript#38603);
 * these cover only what this package touches: the WebCodecs encoders and
 * decoders, their input/output chunk types, VideoFrame, and AudioData.
 */
declare global {
  // ── Video ────────────────────────────────────────────────────────────────
  type EncodedVideoChunkType = "key" | "delta";

  interface EncodedVideoChunkInit {
    type: EncodedVideoChunkType;
    timestamp: number; // microseconds
    duration?: number; // microseconds
    data: BufferSource;
  }

  class EncodedVideoChunk {
    constructor(init: EncodedVideoChunkInit);
    readonly type: EncodedVideoChunkType;
    readonly timestamp: number; // microseconds
    readonly byteLength: number;
    copyTo(destination: AllowSharedBufferSource): void;
  }

  interface EncodedVideoChunkMetadata {
    decoderConfig?: VideoDecoderConfig;
  }

  interface VideoDecoderConfig {
    codec: string;
    description?: AllowSharedBufferSource;
    codedWidth?: number;
    codedHeight?: number;
  }

  interface VideoDecoderInit {
    output(frame: VideoFrame): void;
    error(error: DOMException): void;
  }

  class VideoDecoder {
    static isConfigSupported(
      config: VideoDecoderConfig
    ): Promise<{ supported: boolean; config?: VideoDecoderConfig }>;
    constructor(init: VideoDecoderInit);
    readonly state: "unconfigured" | "configured" | "closed";
    configure(config: VideoDecoderConfig): void;
    decode(chunk: EncodedVideoChunk): void;
    flush(): Promise<void>;
    close(): void;
  }

  interface VideoEncoderConfig {
    codec: string;
    width: number;
    height: number;
    bitrate?: number;
    framerate?: number;
    avc?: { format: "avc" | "annexb" };
  }

  interface VideoEncoderEncodeOptions {
    keyFrame?: boolean;
  }

  interface VideoEncoderInit {
    output(chunk: EncodedVideoChunk, metadata?: EncodedVideoChunkMetadata): void;
    error(error: DOMException): void;
  }

  class VideoEncoder {
    static isConfigSupported(
      config: VideoEncoderConfig
    ): Promise<{ supported: boolean; config?: VideoEncoderConfig }>;
    constructor(init: VideoEncoderInit);
    readonly state: "unconfigured" | "configured" | "closed";
    configure(config: VideoEncoderConfig): void;
    encode(frame: VideoFrame, options?: VideoEncoderEncodeOptions): void;
    flush(): Promise<void>;
    close(): void;
  }

  class VideoFrame {
    constructor(data: BufferSource, init: VideoFrameBufferInit);
    readonly codedWidth: number;
    readonly codedHeight: number;
    readonly timestamp: number; // microseconds
    close(): void;
  }

  interface VideoFrameBufferInit {
    format: string;
    codedWidth: number;
    codedHeight: number;
    timestamp: number; // microseconds
    duration?: number;
  }

  // ── Audio ────────────────────────────────────────────────────────────────
  type EncodedAudioChunkType = "key" | "delta";

  interface EncodedAudioChunkInit {
    type: EncodedAudioChunkType;
    timestamp: number; // microseconds
    duration?: number; // microseconds
    data: BufferSource;
  }

  class EncodedAudioChunk {
    constructor(init: EncodedAudioChunkInit);
    readonly type: EncodedAudioChunkType;
    readonly timestamp: number; // microseconds
    readonly duration?: number; // microseconds
    readonly byteLength: number;
    copyTo(destination: AllowSharedBufferSource): void;
  }

  interface EncodedAudioChunkMetadata {
    decoderConfig?: AudioDecoderConfig;
  }

  interface AudioDecoderConfig {
    codec: string;
    sampleRate: number;
    numberOfChannels: number;
    description?: AllowSharedBufferSource;
  }

  interface AudioDecoderInit {
    output(data: AudioData): void;
    error(error: DOMException): void;
  }

  class AudioDecoder {
    static isConfigSupported(
      config: AudioDecoderConfig
    ): Promise<{ supported: boolean; config?: AudioDecoderConfig }>;
    constructor(init: AudioDecoderInit);
    readonly state: "unconfigured" | "configured" | "closed";
    configure(config: AudioDecoderConfig): void;
    decode(chunk: EncodedAudioChunk): void;
    flush(): Promise<void>;
    close(): void;
  }

  interface AudioEncoderConfig {
    codec: string;
    sampleRate: number;
    numberOfChannels: number;
    bitrate?: number;
  }

  interface AudioEncoderInit {
    output(chunk: EncodedAudioChunk, metadata?: EncodedAudioChunkMetadata): void;
    error(error: DOMException): void;
  }

  class AudioEncoder {
    static isConfigSupported(
      config: AudioEncoderConfig
    ): Promise<{ supported: boolean; config?: AudioEncoderConfig }>;
    constructor(init: AudioEncoderInit);
    readonly state: "unconfigured" | "configured" | "closed";
    configure(config: AudioEncoderConfig): void;
    encode(data: AudioData): void;
    flush(): Promise<void>;
    close(): void;
  }

  class AudioData {
    constructor(init: AudioDataInit);
    readonly sampleRate: number;
    readonly numberOfFrames: number;
    readonly numberOfChannels: number;
    readonly timestamp: number; // microseconds
    close(): void;
  }

  interface AudioDataInit {
    format: string;
    sampleRate: number;
    numberOfFrames: number;
    numberOfChannels: number;
    timestamp: number; // microseconds
    data: BufferSource;
  }
}

export {};
