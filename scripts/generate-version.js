#!/usr/bin/env node

/**
 * Generate Version Hash
 * 
 * Creates a SHA256 hash of all files in the _site directory,
 * appends a timestamp, and writes to _site/version.txt
 * 
 * Usage: node scripts/generate-version.js
 */

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');

// Configuration
const SITE_DIRECTORY = path.join(__dirname, '..', '_site');
const VERSION_FILE = path.join(SITE_DIRECTORY, 'version.txt');

// Files/directories to exclude from hashing
const EXCLUDED = new Set([
  'version.txt',
  '.DS_Store',
  '.git',
  '.gitignore',
  'node_modules'
]);

/**
 * Check if a file should be excluded from hashing
 */
function shouldExclude(filename) {
  return EXCLUDED.has(filename) || filename.startsWith('.');
}

/**
 * Recursively get all files in a directory
 */
function getAllFiles(dir, files = []) {
  const items = fs.readdirSync(dir);
  
  for (const item of items) {
    if (shouldExclude(item)) {
      continue;
    }
    
    const fullPath = path.join(dir, item);
    const stat = fs.statSync(fullPath);
    
    if (stat.isDirectory()) {
      getAllFiles(fullPath, files);
    } else {
      files.push(fullPath);
    }
  }
  
  return files;
}

/**
 * Generate SHA256 hash of all file contents
 */
function generateDirHash(directory) {
  const hash = crypto.createHash('sha256');
  
  // Get all files and sort them for consistent hashing
  const files = getAllFiles(directory).sort();
  
  console.log(`Hashing ${files.length} files...`);
  
  for (const file of files) {
    // Add file path relative to site directory for consistency
    const relativePath = path.relative(directory, file);
    hash.update(relativePath);
    
    // Add file content
    const content = fs.readFileSync(file);
    hash.update(content);
  }
  
  return hash.digest('hex');
}

/**
 * Main execution
 */
function main() {
  try {
    // Check if _site directory exists
    if (!fs.existsSync(SITE_DIRECTORY)) {
      console.error(`Error: Directory '${SITE_DIRECTORY}' does not exist.`);
      console.error('Make sure to build your site first (e.g., jekyll build)');
      process.exit(1);
    }
    
    // Generate hash
    const dirHash = generateDirHash(SITE_DIRECTORY);
    
    // Add timestamp
    const timestamp = new Date().toISOString()
      .replace(/[:.]/g, '-')
      .replace('T', '_')
      .slice(0, -5); // Remove milliseconds and Z
    
    const version = `${dirHash}_${timestamp}`;
    
    // Ensure _site directory exists (it should, but just in case)
    if (!fs.existsSync(SITE_DIRECTORY)) {
      fs.mkdirSync(SITE_DIRECTORY, { recursive: true });
    }
    
    // Write version file
    fs.writeFileSync(VERSION_FILE, version + '\n');
    
    console.log(`✓ New version generated: ${version}`);
    console.log(`✓ Written to: ${VERSION_FILE}`);
    
  } catch (error) {
    console.error('Error generating version:', error.message);
    process.exit(1);
  }
}

main();
