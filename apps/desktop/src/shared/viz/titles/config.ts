import cfg from "../../../../../../assets/viz/titles.json";

/** `assets/viz/titles.json` (D-122): every number of the «Титры» mode lives there. */
export type StyleId = "shock" | "poster";
export type TitlesStylePref = "auto" | StyleId;
export type TitlesQuality = {
  grain: boolean;
  halftone: boolean;
  glitch: boolean;
  flicker: boolean;
};

export type TitlesConfig = {
  fps: Record<"1" | "2" | "3" | "4", number>;
  pixel_scale: Record<"1" | "2" | "3" | "4", number>;
  quality: Record<"1" | "2" | "3" | "4", TitlesQuality>;
  card: {
    min_s: number;
    max_s: number;
    beats_per_bar: number;
    enter_frames: [number, number];
    fallback_bpm: number;
  };
  acts: {
    prologue_until: number;
    climax_from: number;
    density: { prologue: number; ramp: number; climax: number };
    energy_gain: number;
  };
  rest: { energy_below: number; hold_ms: number };
  onset: {
    ema: number;
    ratio: number;
    floor: number;
    rise: number;
    min_gap_ms: number;
    kick_ms: number;
    snare_ms: number;
  };
  step: { max_hz: number };
  flash: { min_delta: number; min_area: number };
  auto: { bpm_split: number; arousal_split: number };
  text: { xscale_wide: number; xscale_narrow: number; role_size: number; max_lines: number };
  scrim: { shock: number; poster: number; auto: number };
  /** Decoration in Japanese (D-123): only words, marks and shapes — the colours are the style's own. */
  japan: {
    roles: Record<"title" | "artist" | "tempo" | "mood" | "album" | "year" | "genre", string>;
    words: string[];
    tags: string[];
    seals: string[];
    numerals: string[];
    episode: [string, string];
    column_size: number;
    tag_size: number;
    seal_size: number;
  };
  styles: {
    shock: {
      field: string;
      ink: string;
      mass: string;
      texture: string;
      accents: string[];
      gradients: [string, string][];
      marker: string;
      text_on_field: string;
      slits: { angle_deg: number; max_area: number };
      ink_coverage: number;
    };
    poster: {
      field: string;
      grays: string[];
      frame_steps: string[];
      text: string;
      pulse: string;
      white: string;
      frame_width: number;
      grain: number;
    };
  };
};

export const TITLES = cfg as unknown as TitlesConfig;
export type LevelKey = "1" | "2" | "3" | "4";
export const levelKey = (level: number): LevelKey =>
  String(Math.min(4, Math.max(1, Math.round(level)))) as LevelKey;
