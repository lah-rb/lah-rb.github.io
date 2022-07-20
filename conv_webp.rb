require 'fileutils'
include FileUtils

full_site_images = Dir['assets/images/*']
thumbnail_images = Dir['assets/thumbnails/*']
png_site_images = full_site_images #+ thumbnail_images
png_site_images.keep_if { |image| image.split('.')[1] == 'png' }
png_site_images.each do |image|
  path_without_suffix = image.split('.')
  system("cwebp -q 80 #{path_without_suffix[0]}.png -o #{path_without_suffix[0]}.webp -mt")
  rm image
end
