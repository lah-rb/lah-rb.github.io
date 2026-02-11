const fs = require('fs');
const { JSDOM } = require('jsdom');
const TurndownService = require('turndown');

const html = fs.readFileSync('rules.html', 'utf-8');
const dom = new JSDOM(html);
const doc = dom.window.document;

const turndown = new TurndownService({
  headingStyle: 'atx',
  bulletListMarker: '-',
  codeBlockStyle: 'fenced',
  emDelimiter: '*',
  strongDelimiter: '**',
});

// Custom rule for definition lists
turndown.addRule('definitionList', {
  filter: 'dl',
  replacement: function(content, node) {
    let result = '\n';
    const items = node.children;
    for (let i = 0; i < items.length; i++) {
      const child = items[i];
      if (child.tagName === 'DT') {
        const dtText = turndown.turndown(child.innerHTML).replace(/<\/?dd>/g, '').trim();
        // Check if DT contains DD children directly
        const dds = child.querySelectorAll('dd');
        if (dds.length > 0) {
          // DT has nested DDs
          const dtOnly = child.childNodes[0]?.textContent?.trim() || '';
          result += `\n**${dtOnly}**\n`;
          dds.forEach(dd => {
            result += `: ${turndown.turndown(dd.innerHTML).trim()}\n`;
          });
        } else {
          result += `\n**${dtText}**\n`;
        }
      } else if (child.tagName === 'DD') {
        result += `: ${turndown.turndown(child.innerHTML).trim()}\n`;
      } else if (child.tagName === 'DIV') {
        // Some DTs/DDs are wrapped in divs
        const innerDts = Array.from(child.querySelectorAll('dt'));
        const innerDds = Array.from(child.querySelectorAll('dd'));
        innerDts.forEach(dt => {
          const ddsInDt = Array.from(dt.querySelectorAll('dd'));
          if (ddsInDt.length > 0) {
            const label = dt.childNodes[0]?.textContent?.trim() || '';
            result += `\n**${label}**\n`;
            ddsInDt.forEach(dd => {
              result += `: ${turndown.turndown(dd.innerHTML).trim()}\n`;
            });
          } else {
            result += `\n**${turndown.turndown(dt.innerHTML).trim()}**\n`;
          }
        });
        innerDds.forEach(dd => {
          if (!dd.parentElement || dd.parentElement.tagName !== 'DT') {
            result += `: ${turndown.turndown(dd.innerHTML).trim()}\n`;
          }
        });
      }
    }
    return result + '\n';
  }
});

// Custom rule for tables
turndown.addRule('table', {
  filter: 'table',
  replacement: function(content, node) {
    const rows = Array.from(node.querySelectorAll('tr'));
    const caption = node.querySelector('caption');
    let result = '\n';
    
    if (caption) {
      result += `**${caption.textContent.trim()}**\n\n`;
    }
    
    // Get headers
    const thRow = node.querySelector('th');
    let headers = [];
    if (thRow) {
      const thCells = Array.from(thRow.querySelectorAll('td, b'));
      thCells.forEach(cell => {
        const text = cell.textContent.trim();
        if (text) headers.push(text);
      });
    }
    
    if (headers.length > 0) {
      result += '| ' + headers.join(' | ') + ' |\n';
      result += '| ' + headers.map(() => '---').join(' | ') + ' |\n';
    }
    
    rows.forEach(row => {
      const cells = Array.from(row.querySelectorAll('td'));
      const cellTexts = [];
      cells.forEach(cell => {
        const text = cell.textContent.trim();
        if (text) cellTexts.push(text);
      });
      if (cellTexts.length > 0 && cellTexts.some(t => t.length > 0)) {
        result += '| ' + cellTexts.join(' | ') + ' |\n';
      }
    });
    
    return result + '\n';
  }
});

// Remove SVG elements (icons)
turndown.addRule('removeSvg', {
  filter: 'svg',
  replacement: () => ''
});

// Handle images
turndown.addRule('images', {
  filter: 'img',
  replacement: function(content, node) {
    const alt = node.getAttribute('alt') || '';
    const src = node.getAttribute('src') || '';
    return `![${alt}](${src})`;
  }
});

let output = '';

// Process top-level sections
const topSections = doc.querySelectorAll('.slides > section');

topSections.forEach(section => {
  const sectionId = section.getAttribute('id') || '';
  const sectionName = section.getAttribute('data-name') || '';
  
  // Check if this section has nested sections (slides)
  const nestedSections = section.querySelectorAll(':scope > section');
  
  if (nestedSections.length > 0) {
    // This is a chapter with sub-slides
    let headerDone = false;
    
    nestedSections.forEach((subSection, idx) => {
      const subId = subSection.getAttribute('id') || '';
      
      // Process content of each sub-section
      const children = subSection.children;
      for (let i = 0; i < children.length; i++) {
        const el = children[i];
        const tag = el.tagName.toLowerCase();
        
        if (tag === 'h2') {
          const headerId = subId || sectionId;
          const text = el.textContent.trim();
          output += `## ${text} {#${headerId}}\n\n`;
          headerDone = true;
        } else if (tag === 'h3') {
          const headerId = subId || '';
          const text = el.textContent.trim();
          if (headerId) {
            output += `### ${text} {#${headerId}}\n\n`;
          } else {
            output += `### ${text}\n\n`;
          }
        } else if (tag === 'h4') {
          output += `#### ${el.textContent.trim()}\n\n`;
        } else {
          const md = turndown.turndown(el.outerHTML).trim();
          if (md) {
            output += md + '\n\n';
          }
        }
      }
      
      output += '\n';
    });
  } else {
    // Single section (no nested slides)
    const children = section.children;
    for (let i = 0; i < children.length; i++) {
      const el = children[i];
      const tag = el.tagName.toLowerCase();
      
      if (tag === 'h2') {
        const headerId = sectionId || '';
        const text = el.textContent.trim();
        if (headerId) {
          output += `## ${text} {#${headerId}}\n\n`;
        } else {
          output += `## ${text}\n\n`;
        }
      } else if (tag === 'h3') {
        const text = el.textContent.trim();
        output += `### ${text}\n\n`;
      } else {
        const md = turndown.turndown(el.outerHTML).trim();
        if (md) {
          output += md + '\n\n';
        }
      }
    }
    output += '\n';
  }
});

// Clean up excessive newlines
output = output.replace(/\n{4,}/g, '\n\n\n');

fs.writeFileSync('rules.md', output, 'utf-8');
console.log('Conversion complete! Output: rules.md');
console.log('Output size:', output.length, 'characters');
