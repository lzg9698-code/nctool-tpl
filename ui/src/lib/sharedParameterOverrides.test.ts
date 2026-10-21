import { describe, expect, it } from "vitest";
import { DraftNumber, encode, newDocument, numberText, parseData } from "./data";
import { managedRequest, mergeParameterLayers, overridePointer, removePointer, setParameterOverride } from "./managedParameters";

describe("explicit shared parameter overrides", () => {
  it("keeps equal explicit values without freezing inherited siblings", () => {
    const inherited = parseData('{"nested":{"width":25,"other":9007199254740993}}');
    const next = setParameterOverride({}, inherited, "/nested/width", parseData("25"));
    expect(encode(next)).toBe('{"nested":{"width":25}}');
    expect(encode(mergeParameterLayers(inherited, next))).toBe('{"nested":{"width":25,"other":9007199254740993}}');
  });
  it("keeps invalid numeric drafts stable across requests, merging, and subsequent edits", () => {
    const doc = newDocument();
    doc.context = { width: new DraftNumber("1e-") };
    const request = managedRequest(doc);
    expect(numberText(request.values.width)).toBe("1e-");
    expect(() => encode(request)).toThrow("完整");
    const next = setParameterOverride(doc.context, { width: new DraftNumber("1e-") }, "/other", "x");
    expect(numberText(next.width)).toBe("1e-");
    expect(numberText(mergeParameterLayers({ width: 1 }, next).width)).toBe("1e-");
  });
  it("writes array descendants atomically and restores the entire array layer", () => {
    const effective = parseData('{"rows":[{"depth":9007199254740993},{"depth":2}]}');
    const next = setParameterOverride({}, effective, "/rows/1/depth", new DraftNumber("3"));
    expect(encode(next)).toBe('{"rows":[{"depth":9007199254740993},{"depth":3}]}');
    expect(overridePointer(next, "/rows/1/depth")).toBe("/rows");
    expect(removePointer(next, overridePointer(next, "/rows/1/depth"))).toEqual({});
    expect(setParameterOverride(next, effective, "/rows/0", undefined)).toEqual({});
    expect(encode(effective)).toBe('{"rows":[{"depth":9007199254740993},{"depth":2}]}');
  });
  it("supports escaped paths and treats numeric-looking object keys as objects", () => {
    const next = setParameterOverride({}, { "a/b": { "0": 1 } }, "/a~1b/0", new DraftNumber("2"));
    expect(encode(next)).toBe('{"a/b":{"0":2}}');
    expect(overridePointer(next, "/a~1b/0")).toBe("/a~1b/0");
  });
  it("writes prototype-named properties as own data without mutating prototypes", () => {
    const next = setParameterOverride({}, {}, "/__proto__/polluted", "safe");
    expect(Object.hasOwn(next, "__proto__")).toBe(true);
    expect(({} as any).polluted).toBeUndefined();
    expect(encode(next)).toBe('{"__proto__":{"polluted":"safe"}}');
  });
});
