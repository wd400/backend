words=[]
with open("bad_words.txt") as f:
    lines=f.readlines()
    cleaned=[]
    for line in lines:
        line=line.lower().strip()
        if line.isalnum():
            cleaned.append(line)
    cleaned.sort(key=lambda x:len(x))
    for clean in cleaned:
        to_add=True
        for word in words:
            if word in clean:
                to_add=False
                break
        if to_add:
            words.append(clean)
print('|'.join(words))