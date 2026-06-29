// A small TypeScript example: a typed Result type and a safe parser.

interface Ok<T> {
    ok: true;
    value: T;
}

interface Err {
    ok: false;
    error: string;
}

type Result<T> = Ok<T> | Err;

function parsePositiveInt(input: string): Result<number> {
    const n = Number(input);
    if (!Number.isInteger(n) || n <= 0) {
        return { ok: false, error: `"${input}" is not a positive integer` };
    }
    return { ok: true, value: n };
}

const samples: string[] = ["42", "-1", "3.14", "100"];

for (const s of samples) {
    const result = parsePositiveInt(s);
    if (result.ok) {
        console.log(`Parsed: ${result.value}`);
    } else {
        console.error(`Error: ${result.error}`);
    }
}
