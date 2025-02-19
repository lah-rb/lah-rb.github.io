#!/usr/bin/env ruby

require 'digest/sha2'
require 'find'

def dir_sha256(directory)
  # Initialize the hash
  sha256 = Digest::SHA256.new

  # Iterate over all files in the directory and update the hash
  Find.find(directory) do |path|
    next if File.directory?(path) # Skip directories themselves
    sha256.update(File.read(path))
  end

  sha256.hexdigest
end

site_directory = '/home/lah-rb/Repos/lah-rb.github.io/_site'
work_directory = '/home/lah-rb/Repos/lah-rb.github.io'
new_version = dir_sha256(site_directory) + Time.now.strftime("_%Y-%m-%d_%H-%M-%S")

# Write the new version to version.txt
File.open("#{work_directory}/version.txt", 'w') { |f| f.puts new_version }

puts "New version generated: #{new_version}"
