// Global references
let deck = Reveal;

let searchElement;
let searchInput;
let resultNavContainer;
let prevResultButton;
let nextResultButton;

let matchedSlides;
let currentMatchedIndex;
let searchboxDirty;
let hilitor;

function render() {
    const maxAttempts = 10; // Maximum attempts to find the element
    const attemptInterval = 200; // Interval between attempts in milliseconds
    let attemptCount = 0;

    function checkForSearchContainer() {
        const searchContainer = document.getElementById('search-container');
        
        if (searchContainer) {
            initializeSearch(searchContainer);
        } else if (attemptCount < maxAttempts) {
            attemptCount++;
            setTimeout(checkForSearchContainer, attemptInterval);
        } else {
            console.error("Failed to find #search-container after", maxAttempts, "attempts.");
        }
    }

    function initializeSearch(searchContainer) {
        // Assuming you have a div with id 'search-container' in your HTML where the search input should be rendered
        if (!searchElement) { // Check if already initialized
            searchElement = document.createElement('div');
            // Optional: Add class for styling the search box within its container
            searchElement.classList.add('searchbox-inside');

            searchElement.innerHTML = `
              <input type="search" class="searchinput" placeholder="Search the Rules" />
              <div class="result-nav-container" style="display:none;">
                <button class="prev-result">Previous</button>
                <button class="next-result">Next</button>
              </div>
            `;

            searchContainer.appendChild(searchElement);

            searchInput = searchElement.querySelector('.searchinput');
            resultNavContainer = searchElement.querySelector('.result-nav-container');
            prevResultButton = resultNavContainer.querySelector('.prev-result');
            nextResultButton = resultNavContainer.querySelector('.next-result');

            // Event listeners for navigation buttons
            prevResultButton.addEventListener('click', navigateToPreviousResult);
            nextResultButton.addEventListener('click', doSearch); // Reuses the doSearch function to navigate forward

            // Keyboard event listener
            searchInput.addEventListener('keyup', function(event) {
                switch (event.keyCode) {
                    case 13:
                        event.preventDefault();
                        doSearch();
                        searchboxDirty = false;
                        break;
                    default:
                        searchboxDirty = true;
                }
            }, false);
        }
    }

    checkForSearchContainer();
}


function openSearch() {
    if (!searchElement) render();

    // Ensure visibility
    searchElement.style.display = 'block';
    resultNavContainer.style.display = 'none'; // Hide on opening, show when results are found
    searchInput.focus();
    searchInput.select();
}

function closeSearch() {
    if (!searchElement) render();

    searchElement.style.display = 'none';
    if (hilitor) hilitor.remove();
}

function toggleSearch() {
    if (!searchElement) render();

    if (searchElement.style.display !== 'block') {
        openSearch();
    } else {
        closeSearch();
    }
}

function doSearch() {
    //if there's been a change in the search term, perform a new search:
    if (searchboxDirty) {
        var searchstring = searchInput.value;

        if (searchstring === '') {
            if (hilitor) hilitor.remove();
            matchedSlides = null;
            resultNavContainer.style.display = 'none'; // Hide navigation on empty query
            searchInput.value = ''; // Reset search input
        } else {
            //find the keyword amongst the slides
            hilitor = new Hilitor("slidecontent");
            matchedSlides = hilitor.apply(searchstring);
            currentMatchedIndex = 0;
            
            if (matchedSlides.length > 0) { // Show navigation and update slide if there are results
                resultNavContainer.style.display = 'block';
                deck.slide(matchedSlides[currentMatchedIndex].h, matchedSlides[currentMatchedIndex].v);
                searchInput.value = searchstring; // Ensure the original query remains in the input
            } else { // No results found scenario
                resultNavContainer.style.display = 'none';
                searchInput.value = 'No results found...'; // Display message
                setTimeout(function() {
                    if (searchInput.value === 'No results found...') { // Reset after a brief period if unchanged
                        searchInput.value = searchstring; // Restore original query
                    }
                }, 2000); // Brief display period (2 seconds)
            }
        }
    }

    // Navigation logic when results exist remains the same
    if (matchedSlides && matchedSlides.length > currentMatchedIndex) {
        deck.slide(matchedSlides[currentMatchedIndex].h, matchedSlides[currentMatchedIndex].v);
        currentMatchedIndex++;
        // Update button states or text if desired (e.g., disabling at ends)
    }
}

function navigateToPreviousResult() {
    if (matchedSlides && matchedSlides.length > 0) {
        if (currentMatchedIndex <= 1) { // Considering 1 as the first valid index after a search
            currentMatchedIndex = matchedSlides.length;
        } else {
            currentMatchedIndex--;
        }
        deck.slide(matchedSlides[currentMatchedIndex - 1].h, matchedSlides[currentMatchedIndex - 1].v);
    }
}

// Hilitor logic for highlighting search results
function Hilitor(id, tag) {

    var targetNode = document.getElementById(id) || document.body;
    var hiliteTag = tag || "EM";
    var skipTags = new RegExp("^(?:" + hiliteTag + "|SCRIPT|FORM)$");
    var colors = ["#ff6", "#a0ffff", "#9f9", "#f99", "#f6f"];
    var wordColor = [];
    var colorIdx = 0;
    var matchRegex = "";
    var matchingSlides = [];

    this.setRegex = function(input) {
        input = input.trim();
        matchRegex = new RegExp("(" + input + ")", "i");
    }

    this.getRegex = function() {
        return matchRegex.toString().replace(/^\/\\b\(|\)\\b\/i$/g, "").replace(/\|/g, " ");
    }

    // recursively apply word highlighting
    this.hiliteWords = function(node) {
        if (node == undefined || !node) return;
        if (!matchRegex) return;
        if (skipTags.test(node.nodeName)) return;

        if (node.hasChildNodes()) {
            for (var i = 0; i < node.childNodes.length; i++)
                this.hiliteWords(node.childNodes[i]);
        }
        if (node.nodeType == 3) { // NODE_TEXT
            var nv, regs;
            if ((nv = node.nodeValue) && (regs = matchRegex.exec(nv))) {
                //find the slide's section element and save it in our list of matching slides
                var secnode = node;
                while (secnode != null && secnode.nodeName != 'SECTION') {
                    secnode = secnode.parentNode;
                }

                var slideIndex = deck.getIndices(secnode);
                var slidelen = matchingSlides.length;
                var alreadyAdded = false;
                for (var i = 0; i < slidelen; i++) {
                    if ((matchingSlides[i].h === slideIndex.h) && (matchingSlides[i].v === slideIndex.v)) {
                        alreadyAdded = true;
                    }
                }
                if (!alreadyAdded) {
                    matchingSlides.push(slideIndex);
                }

                if (!wordColor[regs[0].toLowerCase()]) {
                    wordColor[regs[0].toLowerCase()] = colors[colorIdx++ % colors.length];
                }

                var match = document.createElement(hiliteTag);
                match.appendChild(document.createTextNode(regs[0]));
                match.style.backgroundColor = wordColor[regs[0].toLowerCase()];
                match.style.fontStyle = "inherit";
                match.style.color = "#000";

                var after = node.splitText(regs.index);
                after.nodeValue = after.nodeValue.substring(regs[0].length);
                node.parentNode.insertBefore(match, after);
            }
        }
    };

    // remove highlighting
    this.remove = function() {
        var arr = document.getElementsByTagName(hiliteTag);
        var el;
        while (arr.length && (el = arr[0])) {
            el.parentNode.replaceChild(el.firstChild, el);
        }
    };

    // start highlighting at target node
    this.apply = function(input) {
        if (input == undefined || !input) return;
        this.remove();
        this.setRegex(input);
        this.hiliteWords(targetNode);
        return matchingSlides;
    };

}

// Initialize the search functionality when the document is ready
document.addEventListener('DOMContentLoaded', function(event) {
    render();

    // Optionally, add a keyboard shortcut to toggle the search (e.g., CTRL + Shift + F)
    document.addEventListener('keydown', function(event) {
        if ((event.key == "F" || event.key == "f") && (event.ctrlKey || event.metaKey)) { //Control+Shift+f
            event.preventDefault();
            toggleSearch();
        }
    }, false);
});
