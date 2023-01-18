pip install pdoc3
rm -rf docs/resources
pdoc --skip-errors --template-dir docs/pdoc_template -o docs/docs/resources client/src/bastionlab
sed -i '/<p>Generated by <a href="https:\/\/pdoc3.github.io\/pdoc" title="pdoc: Python API documentation generator"><cite>pdoc<\/cite> 0.10.0<\/a>.<\/p>/d' docs/docs/resources/bastionlab/*.md docs/docs/resources/bastionlab/*/*.md

# Fix broken links in the generated documentation
bash docs/fix_links.sh
# Generate the structure of the documentation for mkdocs.yml file
python docs/docs/gen_ref_pages.py