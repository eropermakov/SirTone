import { describe, expect, it } from "vitest";
import { blankDraft, draftToJson, jsonToDraft } from "./smartDraft";

describe("smart playlist draft ↔ rules JSON (T3.13)", () => {
  it("round-trips groups, sort, limit and refresh", () => {
    const json =
      '{"all":[{"contains":{"title":"лето"}},{"any":[{"gt":{"year":2000}},{"is":{"liked":true}}]}],"sort":"-play_count","limit":50,"refresh":"12h"}';
    const d = jsonToDraft(json);
    expect(d.sort).toBe("play_count");
    expect(d.desc).toBe(true);
    expect(JSON.parse(draftToJson(d))).toEqual(JSON.parse(json));
  });

  it("keeps conditions the editor cannot show", () => {
    const json = '{"all":[{"inPlaylist":{"id":3}},{"isMissing":{"year":true}}],"refresh":"live"}';
    expect(JSON.parse(draftToJson(jsonToDraft(json)))).toEqual(JSON.parse(json));
  });

  it("an empty number stays empty (Rust refuses it) instead of 0", () => {
    const d = blankDraft();
    d.root.items = [{ type: "cond", field: "year", op: "gt", value: "" }];
    expect(JSON.parse(draftToJson(d)).all[0]).toEqual({ gt: { year: "" } });
  });
});
