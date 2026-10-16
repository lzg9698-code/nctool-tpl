// @vitest-environment jsdom
import { cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Form } from "./Form";
import { encode } from "../lib/data";

afterEach(cleanup);
describe("explicit field edits", () => {
  it("reports an equal value and escapes nested field names without freezing siblings", () => {
    const changed = vi.fn();
    const whole = vi.fn();
    const schema = {type:"object",properties:{report:{type:"object",properties:{"a/b~c":{type:"string"},other:{type:"string"}}}}};
    const { container, rerender } = render(<Form schema={schema} value={{report:{"a/b~c":"same",other:"inherited"}}} onChange={whole} onFieldChange={changed}/>);
    fireEvent.change(container.querySelector('input')!, {target:{value:"edited"}});
    rerender(<Form schema={schema} value={{report:{"a/b~c":"edited",other:"inherited"}}} onChange={whole} onFieldChange={changed}/>);
    fireEvent.change(container.querySelector('input')!, {target:{value:"same"}});
    expect(changed.mock.calls.at(-1)).toEqual(["/report/a~1b~0c", "same"]);
    expect(whole).toHaveBeenCalledWith({report:{"a/b~c":"same",other:"inherited"}});
  });
  it("reports array item edits atomically at the array root", () => {
    const changed = vi.fn();
    const { container } = render(<Form schema={{type:"object",properties:{rows:{type:"array",items:{type:"number"}}}}} value={{rows:[1,2]}} onChange={()=>{}} onFieldChange={changed}/>);
    fireEvent.change(container.querySelector('input')!, {target:{value:"9"}});
    expect(changed.mock.calls[0][0]).toBe("/rows");
    expect(encode(changed.mock.calls[0][1])).toBe("[9,2]");
    expect(changed).toHaveBeenCalledTimes(1);
  });
  it("reports dictionary additions with RFC6901 paths", () => {
    const changed = vi.fn();
    const { getByLabelText } = render(<Form schema={{type:"object"}} value={{}} onChange={()=>{}} onFieldChange={changed}/>);
    fireEvent.change(getByLabelText("字段名"), {target:{value:"a/b"}});
    fireEvent.click(getByLabelText("添加字段"));
    expect(changed).toHaveBeenCalledWith("/a~1b", "");
  });
});
