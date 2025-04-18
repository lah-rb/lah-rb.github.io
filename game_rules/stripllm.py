import os
from bs4 import BeautifulSoup, NavigableString
import nltk
from nltk.tokenize import word_tokenize
nltk.download('punkt')  # Ensure Punkt tokenizer models are downloaded
nltk.download('punkt_tab')


# Constants
INPUT_FILE = "./rules.html"
OUTPUT_FILE = "./llmrules.txt"

def count_tokens(text):
    """Return the number of tokens in the given text."""
    return len(word_tokenize(text))

def strip_html(input_file, output_file):
    """Strip HTML, reduce tokens, and save to output file, reporting token savings."""
    with open(input_file, 'r', encoding='utf-8') as file:
        soup = BeautifulSoup(file, 'html.parser')

        # Remove non-essential tags
        for tag in soup.find_all(['script', 'style', 'nav']):
            tag.decompose()

        # Extract and consolidate essential text
        text_nodes = []
        for element in soup.find_all(['p', 'li', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6']):
            if isinstance(element, NavigableString):
                text_nodes.append(str(element))
            else:
                text_nodes.append(element.get_text())

        # Join text nodes and replace HTML entities
        original_text = soup.get_text()
        stripped_text = ' '.join(text_nodes).replace('&nbsp;', ' ').replace('&amp;', '&')

        # Token Savings Metric
        original_tokens = count_tokens(original_text)
        stripped_tokens = count_tokens(stripped_text)
        token_savings = ((original_tokens - stripped_tokens) / original_tokens) * 100 if original_tokens > 0 else 0

        print(f"Original Tokens: {original_tokens}")
        print(f"Stripped Tokens: {stripped_tokens}")
        print(f"Token Savings: {token_savings:.2f}%")

        # Save stripped text to output file
        with open(output_file, 'w', encoding='utf-8') as outfile:
            outfile.write(stripped_text)

if __name__ == "__main__":
    strip_html(INPUT_FILE, OUTPUT_FILE)
    print("Processing Complete. Check output file for results.")
