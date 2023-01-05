pip install pdoc3
rm -rf docs/resources
pdoc --html --skip-errors --template-dir docs/pdoc_template -o docs/docs/resources client/src/bastionlab
sed -i '/<p>Generated by <a href="https:\/\/pdoc3.github.io\/pdoc" title="pdoc: Python API documentation generator"><cite>pdoc<\/cite> 0.10.0<\/a>.<\/p>/d' docs/docs/resources/bastionlab/*.html docs/docs/resources/bastionlab/*/*.html
apt-get install npm

npm install -g to-markdown-cli

for file in docs/docs/resources/bastionlab/*.html; do
    html2md -i $file -o ${file%.html}.md
done