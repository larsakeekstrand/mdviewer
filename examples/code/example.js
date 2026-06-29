// A small JavaScript example: debounce utility with a usage demo.

/**
 * Returns a function that delays invoking `fn` until after `wait` ms
 * have elapsed since the last call.
 */
function debounce(fn, wait = 200) {
    let timer = null;
    return function (...args) {
        clearTimeout(timer);
        timer = setTimeout(() => fn.apply(this, args), wait);
    };
}

const GREETING = "Hello";

const greet = debounce((name) => {
    const message = `${GREETING}, ${name}! (debounced)`;
    console.log(message);
}, 300);

// Rapid calls — only the last one fires after 300 ms.
greet("Alice");
greet("Bob");
greet("world");
