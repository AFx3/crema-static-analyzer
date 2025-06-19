# Number Generator... Without Random Number Generator???

This Rust program generates a number based on numbers that contains your computer's memory (address) instead of traditional random number generation techniques.
## Usage example
https://github.com/user-attachments/assets/f9f60533-d7a4-46dc-aaf0-49a724b6e8a4

## Features

- Generates a number based on numbers that contains your computer's memory.
- Utilizes mathematical constants π (Pi) and φ (Phi) in calculations. (Just why not)
- Ensures the result is != 0 and constrained to be at the range of 1 to 100.

## How It Works

1. The program creates a ```Box``` with a vector that contains a ' '(space) in it.
2. After this program does a little bit of manipulations:<br>
   ```Box --> Raw pointer --> usize --> string --> chars```
3. And the next step is filtering assuring that there is no 0 (Because we need to have the product from them)
4. The product is then multiplied by π and φ. (As mentioned above: "Just why not")
5. The result is taken modulo 101.
6. If the result is zero, it replaces it with a non-zero value (1).


## Usage

To run the program, follow these steps:

1. Clone the repository or copy the code into a new Rust project.
2. Run the following command in the project directory:

   ```bash
   cargo run
****
