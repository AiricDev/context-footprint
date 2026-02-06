import { describe, it, expect } from "bun:test";
import { extractAtomicTypes } from "../src/languages/python/type-flattener";

describe("extractAtomicTypes", () => {
  it("returns simple type as-is", () => {
    expect(extractAtomicTypes("MyClass")).toEqual(["MyClass"]);
  });

  it("unwraps Optional", () => {
    expect(extractAtomicTypes("Optional[MyClass]")).toEqual(["MyClass"]);
  });

  it("unwraps nested Optional with module path", () => {
    expect(extractAtomicTypes("Optional[JuhellmModelConfigAdapter]")).toEqual(["JuhellmModelConfigAdapter"]);
  });

  it("handles PEP 604 union", () => {
    expect(extractAtomicTypes("str | int")).toEqual(["str", "int"]);
  });

  it("handles PEP 604 union with None", () => {
    expect(extractAtomicTypes("MyClass | None")).toEqual(["MyClass"]);
  });

  it("unwraps Union", () => {
    expect(extractAtomicTypes("Union[str, int, MyClass]")).toEqual(["str", "int", "MyClass"]);
  });

  it("unwraps List", () => {
    expect(extractAtomicTypes("List[Request]")).toEqual(["Request"]);
  });

  it("unwraps Dict", () => {
    const result = extractAtomicTypes("Dict[str, MyModel]");
    expect(result).toEqual(["str", "MyModel"]);
  });

  it("unwraps nested generics", () => {
    expect(extractAtomicTypes("Optional[List[MyClass]]")).toEqual(["MyClass"]);
  });

  it("handles callable/lambda signatures", () => {
    expect(extractAtomicTypes("((func: Unknown) -> ((...) -> (Unknown | None)))")).toEqual(["Callable"]);
  });

  it("handles simple callable signature", () => {
    expect(extractAtomicTypes("(int, str) -> bool")).toEqual(["Callable"]);
  });

  it("drops Unknown", () => {
    expect(extractAtomicTypes("Unknown")).toEqual([]);
  });

  it("drops Unknown from composite", () => {
    expect(extractAtomicTypes("Optional[Unknown]")).toEqual([]);
  });

  it("handles _Wrapped types", () => {
    const result = extractAtomicTypes("_Wrapped[SomeType, Unknown, OtherType, Unknown]");
    expect(result).toEqual(["SomeType", "OtherType"]);
  });

  it("handles Final", () => {
    expect(extractAtomicTypes("Final[int]")).toEqual(["int"]);
  });

  it("handles Annotated (first arg only)", () => {
    expect(extractAtomicTypes("Annotated[MyClass, some_metadata]")).toEqual(["MyClass"]);
  });

  it("handles empty string", () => {
    expect(extractAtomicTypes("")).toEqual([]);
  });

  it("handles Tuple", () => {
    expect(extractAtomicTypes("Tuple[str, int, MyClass]")).toEqual(["str", "int", "MyClass"]);
  });

  it("deduplicates results", () => {
    expect(extractAtomicTypes("Union[str, str, int]")).toEqual(["str", "int"]);
  });

  it("handles qualified names", () => {
    expect(extractAtomicTypes("fastapi.Request")).toEqual(["fastapi.Request"]);
  });

  it("drops None and NoneType", () => {
    expect(extractAtomicTypes("NoneType")).toEqual([]);
    expect(extractAtomicTypes("None")).toEqual([]);
  });

  it("handles Callable[..., RetType]", () => {
    expect(extractAtomicTypes("Callable[[int, str], bool]")).toEqual(["Callable"]);
  });
});
