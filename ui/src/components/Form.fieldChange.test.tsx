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

const nullableSchema = {type:"object",properties:{note:{type:["string","null"]}}};
const arraySchema = {type:"object",properties:{rows:{type:"array",items:{type:"string"}}}};

describe("optional-value controls follow disabled and read-only state", () => {
  it("disables filling a null placeholder while disabled", () => {
    const { getByRole } = render(<Form schema={nullableSchema} value={{note:null}} onChange={()=>{}} disabled/>);
    expect((getByRole("button", {name:"填写值"}) as HTMLButtonElement).disabled).toBe(true);
  });
  it("disables the empty-list shortcut while disabled", () => {
    const { getByRole } = render(<Form schema={arraySchema} value={{}} onChange={()=>{}} disabled/>);
    expect((getByRole("button", {name:"设为空列表"}) as HTMLButtonElement).disabled).toBe(true);
  });
  it("disables setting a null value and the sibling clear button while disabled", () => {
    const { getByRole } = render(<Form schema={nullableSchema} value={{note:"x"}} onChange={()=>{}} disabled/>);
    expect((getByRole("button", {name:"设为空值"}) as HTMLButtonElement).disabled).toBe(true);
    expect((getByRole("button", {name:"清空"}) as HTMLButtonElement).disabled).toBe(true);
  });
  it("keeps a read-only field's optional-value buttons disabled", () => {
    const readOnly = {type:"object",properties:{note:{type:["string","null"],readOnly:true}}};
    const { getByRole } = render(<Form schema={readOnly} value={{note:null}} onChange={()=>{}}/>);
    expect((getByRole("button", {name:"填写值"}) as HTMLButtonElement).disabled).toBe(true);
  });
  it("still edits optional values when enabled", () => {
    const whole = vi.fn();
    const changed = vi.fn();
    const { getByRole } = render(<Form schema={arraySchema} value={{}} onChange={whole} onFieldChange={changed}/>);
    fireEvent.click(getByRole("button", {name:"设为空列表"}));
    expect(whole).toHaveBeenCalledWith({rows:[]});
    expect(changed).toHaveBeenCalledWith("/rows", []);
  });
});

describe("labels only point at controls that exist", () => {
  it("drops htmlFor for group and placeholder labels but keeps it for real controls", () => {
    const schema = {type:"object",properties:{
      title:{type:"string"},
      config:{type:"object",properties:{depth:{type:"number"}}},
      rows:{type:"array",items:{type:"string"}},
      note:{type:["string","null"]},
    }};
    const { container } = render(<Form schema={schema} value={{note:null}} onChange={()=>{}}/>);
    for (const label of Array.from(container.querySelectorAll("label[for]"))) {
      expect(document.getElementById(label.getAttribute("for")!)).not.toBeNull();
    }
    const labelFor = (text: string) =>
      Array.from(container.querySelectorAll("label")).find((label) => label.textContent === text);
    expect(labelFor("config")?.getAttribute("for") ?? null).toBeNull();
    expect(labelFor("rows")?.getAttribute("for") ?? null).toBeNull();
    expect(labelFor("note")?.getAttribute("for") ?? null).toBeNull();
    expect(labelFor("title")?.getAttribute("for")).not.toBeNull();
  });
});
