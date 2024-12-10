from setuptools import setup, find_packages

setup(
    name="mrt-lang",
    version="0.1.0",
    description="MRT Programming Language - A modern, expressive programming language",
    long_description=open("README.md").read(),
    long_description_content_type="text/markdown",
    author="MRT Team",
    author_email="your.email@example.com",
    url="https://github.com/yourusername/mrt",
    packages=find_packages(),
    install_requires=[],
    python_requires=">=3.10",  # We use match statements which require Python 3.10+
    entry_points={
        "console_scripts": [
            "mrt=src.__main__:main",
        ],
    },
    classifiers=[
        "Development Status :: 3 - Alpha",
        "Intended Audience :: Developers",
        "License :: OSI Approved :: MIT License",
        "Programming Language :: Python :: 3.10",
        "Programming Language :: Python :: 3.11",
        "Topic :: Software Development :: Interpreters",
    ],
    keywords="programming language interpreter compiler",
)
